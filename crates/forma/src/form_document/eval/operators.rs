//! Operators, member access, calls, interpolation, and pattern matching.

use super::error::EvalError;
use super::{
    Expression, Properties, Value, builtins::call, eval_mode_typed, eval_typed, expression_number,
    number,
};

pub(super) fn eval_expression<'a>(
    expression: &'a Expression,
    scope: &'a Properties,
    state: &'a Properties,
    stack: &mut Vec<&'a str>,
    allow_missing: bool,
) -> Result<Value, EvalError> {
    match expression {
        Expression::Unary(operator, argument) => {
            let argument = eval_mode_typed(argument, scope, state, stack, false)?;
            match operator.as_str() {
                "!" => Ok(Value::Bool(
                    !argument.boolean().map_err(EvalError::TypeError)?,
                )),
                "-" => Ok(number(
                    -expression_number(&argument).map_err(EvalError::TypeError)?,
                )),
                "+" => Ok(number(
                    expression_number(&argument).map_err(EvalError::TypeError)?,
                )),
                _ => Err(EvalError::Message(format!(
                    "Unknown unary operator {operator}"
                ))),
            }
        }
        Expression::Binary(operator, left, right) => {
            let left =
                eval_mode_typed(left, scope, state, stack, operator == "??" || allow_missing)?;
            match operator.as_str() {
                "&&" if !left.boolean().map_err(EvalError::TypeError)? => Ok(Value::Bool(false)),
                "||" if left.boolean().map_err(EvalError::TypeError)? => Ok(Value::Bool(true)),
                "??" if left != Value::Null => Ok(left),
                _ => {
                    let right = eval_mode_typed(right, scope, state, stack, false)?;
                    eval_binary(operator, left, right)
                }
            }
        }
        Expression::Conditional(condition, yes, no) => {
            let condition = eval_mode_typed(condition, scope, state, stack, false)?
                .boolean()
                .map_err(EvalError::TypeError)?;
            eval_mode_typed(if condition { yes } else { no }, scope, state, stack, false)
        }
        Expression::Member(object, property, optional) => eval_member(
            object,
            property,
            *optional || allow_missing,
            scope,
            state,
            stack,
        ),
        Expression::Call(name, arguments) => {
            if name == "coalesce" {
                if arguments.is_empty() {
                    return Err(EvalError::TypeError(
                        "coalesce expects at least one argument".into(),
                    ));
                }
                for arg in arguments {
                    let val = eval_mode_typed(arg, scope, state, stack, true)?;
                    if val != Value::Null {
                        return Ok(val);
                    }
                }
                Ok(Value::Null)
            } else {
                let arguments = arguments
                    .iter()
                    .map(|value| eval_mode_typed(value, scope, state, stack, false))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(call(name, &arguments)?)
            }
        }
    }
}

fn eval_binary(operator: &str, left: Value, right: Value) -> Result<Value, EvalError> {
    match operator {
        "&&" | "||" => Ok(Value::Bool(right.boolean().map_err(EvalError::TypeError)?)),
        "??" => Ok(right),
        "==" | "===" => Ok(Value::Bool(left == right)),
        "!=" | "!==" => Ok(Value::Bool(left != right)),
        ".." | "..=" => {
            let st = expression_number(&left).map_err(EvalError::TypeError)? as i64;
            let en = expression_number(&right).map_err(EvalError::TypeError)? as i64;
            let inclusive = operator == "..=";
            let count = if inclusive { en - st + 1 } else { en - st };
            if count <= 0 {
                Ok(Value::List(vec![]))
            } else if count > 4096 {
                Err(EvalError::RangeExceedsLimit)
            } else if inclusive {
                Ok(Value::List((st..=en).map(|i| number(i as f64)).collect()))
            } else {
                Ok(Value::List((st..en).map(|i| number(i as f64)).collect()))
            }
        }
        "+" if matches!(left, Value::Text(_)) && matches!(right, Value::Text(_)) => {
            Ok(Value::Text(left.text()? + &right.text()?))
        }
        "+" => Ok(number(
            expression_number(&left).map_err(EvalError::TypeError)?
                + expression_number(&right).map_err(EvalError::TypeError)?,
        )),
        "-" => Ok(number(
            expression_number(&left).map_err(EvalError::TypeError)?
                - expression_number(&right).map_err(EvalError::TypeError)?,
        )),
        "*" => Ok(number(
            expression_number(&left).map_err(EvalError::TypeError)?
                * expression_number(&right).map_err(EvalError::TypeError)?,
        )),
        "/" | "%" if expression_number(&right).map_err(EvalError::TypeError)? == 0. => {
            Err(EvalError::DivisionByZero)
        }
        "/" => Ok(number(
            expression_number(&left).map_err(EvalError::TypeError)?
                / expression_number(&right).map_err(EvalError::TypeError)?,
        )),
        "%" => Ok(number(
            expression_number(&left).map_err(EvalError::TypeError)?
                % expression_number(&right).map_err(EvalError::TypeError)?,
        )),
        "<" | "<=" | ">" | ">=" => {
            let order = match (&left, &right) {
                (Value::Text(a), Value::Text(b)) => a.partial_cmp(b),
                _ => expression_number(&left)
                    .map_err(EvalError::TypeError)?
                    .partial_cmp(&expression_number(&right).map_err(EvalError::TypeError)?),
            }
            .ok_or_else(|| EvalError::TypeError("Values cannot be compared".into()))?;
            Ok(Value::Bool(match operator {
                "<" => order.is_lt(),
                "<=" => order.is_le(),
                ">" => order.is_gt(),
                _ => order.is_ge(),
            }))
        }
        _ => Err(EvalError::Message(format!(
            "Unknown binary operator {operator}"
        ))),
    }
}

fn eval_member<'a>(
    object: &'a Value,
    property: &'a Value,
    optional: bool,
    scope: &'a Properties,
    state: &'a Properties,
    stack: &mut Vec<&'a str>,
) -> Result<Value, EvalError> {
    let object = eval_mode_typed(object, scope, state, stack, optional)?;
    if optional && object == Value::Null {
        return Ok(Value::Null);
    }
    let property = eval_mode_typed(property, scope, state, stack, false)?.text()?;
    if ["__proto__", "prototype", "constructor"].contains(&property.as_str()) {
        return Err(EvalError::UnavailableMember(property));
    }
    match object {
        Value::Object(object) => match object.get(&property).cloned() {
            Some(value) => Ok(value),
            None if optional => Ok(Value::Null),
            None => Err(EvalError::UnknownMember(property)),
        },
        Value::List(items) if property == "length" => Ok(number(items.len() as f64)),
        Value::List(items) => match property
            .parse::<usize>()
            .ok()
            .and_then(|index| items.get(index).cloned())
        {
            Some(value) => Ok(value),
            None if optional => Ok(Value::Null),
            None => Err(EvalError::UnknownListIndex(property)),
        },
        Value::Text(value) if property == "length" => {
            Ok(number(value.encode_utf16().count() as f64))
        }
        _ => Err(EvalError::CannotReadMember(property)),
    }
}

pub(super) fn eval_interpolation<'a>(
    parts: &'a [Value],
    scope: &'a Properties,
    state: &'a Properties,
    stack: &mut Vec<&'a str>,
) -> Result<Value, EvalError> {
    let mut out = String::with_capacity(parts.len() * 16);
    for part in parts {
        match eval_typed(part, scope, state, stack)? {
            Value::Null => {}
            Value::Text(v) | Value::Expr(v) => out.push_str(&v),
            Value::Number(v) => {
                let _ = std::fmt::Write::write_fmt(&mut out, format_args!("{v}"));
            }
            Value::Bool(v) => out.push_str(if v { "true" } else { "false" }),
            _ => return Err(EvalError::TypeError("Expected scalar text".into())),
        }
    }
    Ok(Value::Text(out))
}

pub(super) fn pattern_matches_typed<'a>(
    pattern: &'a Value,
    value: &Value,
    scope: &'a Properties,
    state: &'a Properties,
    stack: &mut Vec<&'a str>,
) -> Result<bool, EvalError> {
    if matches!(pattern, Value::Expr(v) if v == "_") {
        return Ok(true);
    }
    if let Value::Object(obj) = pattern {
        if let Some(Value::List(alts)) = obj.get("or") {
            for alt in alts {
                if pattern_matches_typed(alt, value, scope, state, stack)? {
                    return Ok(true);
                }
            }
            return Ok(false);
        }
    }
    if let Value::Expression(expr) = pattern {
        if let Expression::Binary(op, left, right) = expr.as_ref() {
            if op == ".." || op == "..=" {
                let lo = expression_number(&eval_typed(left, scope, state, stack)?)
                    .map_err(EvalError::TypeError)?;
                let hi = expression_number(&eval_typed(right, scope, state, stack)?)
                    .map_err(EvalError::TypeError)?;
                let Value::Number(vn) = value else {
                    return Ok(false);
                };
                return Ok(if op == "..=" {
                    *vn >= lo && *vn <= hi
                } else {
                    *vn >= lo && *vn < hi
                });
            }
        }
    }
    if let Value::List(patterns) = pattern {
        let Value::List(values) = value else {
            return Ok(false);
        };
        if patterns.len() != values.len() {
            return Ok(false);
        }
        for (pattern, value) in patterns.iter().zip(values) {
            if !pattern_matches_typed(pattern, value, scope, state, stack)? {
                return Ok(false);
            }
        }
        return Ok(true);
    }
    Ok(matches_value(
        &eval_typed(pattern, scope, state, stack)?,
        value,
    ))
}

fn matches_value(pattern: &Value, value: &Value) -> bool {
    match (pattern, value) {
        (Value::Expr(v), _) if v == "_" => true,
        (Value::List(a), Value::List(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| matches_value(a, b))
        }
        (Value::Object(pattern), value) if pattern.contains_key("comparison") => {
            let Some(expected) = pattern.get("value") else {
                return false;
            };
            let order = match (value, expected) {
                (Value::Text(a), Value::Text(b)) => a.partial_cmp(b),
                (Value::Number(a), Value::Number(b)) => a.partial_cmp(b),
                _ => None,
            };
            match (
                pattern
                    .get("comparison")
                    .and_then(|v| v.text().ok())
                    .as_deref(),
                order,
            ) {
                (Some("<"), Some(order)) => order.is_lt(),
                (Some("<="), Some(order)) => order.is_le(),
                (Some(">"), Some(order)) => order.is_gt(),
                (Some(">="), Some(order)) => order.is_ge(),
                _ => false,
            }
        }
        _ => pattern == value,
    }
}
