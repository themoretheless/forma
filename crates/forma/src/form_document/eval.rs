use super::{Expression, Properties, Value, number};

mod builtins;
use builtins::call;

pub(super) fn lookup(values: &Properties, path: &str) -> Option<Value> {
    if let Some(value) = values.get(path) {
        return Some(value.clone());
    }
    let (head, tail) = path.split_once('.')?;
    lookup_value(values.get(head)?, tail)
}

pub(super) fn lookup_value(value: &Value, path: &str) -> Option<Value> {
    match value {
        Value::Object(object) => lookup(object, path),
        Value::List(items) if path == "length" => Some(number(items.len() as f64)),
        Value::List(items) => {
            let (index, tail) = path
                .split_once('.')
                .map_or((path, None), |(index, tail)| (index, Some(tail)));
            let value = items.get(index.parse::<usize>().ok()?)?;
            match tail {
                Some(tail) => lookup_value(value, tail),
                None => Some(value.clone()),
            }
        }
        Value::Text(value) if path == "length" => Some(number(value.encode_utf16().count() as f64)),
        _ => None,
    }
}

pub(super) fn enum_value(path: &str, scope: &Properties) -> Option<Value> {
    let (kind, variant) = path.split_once('.')?;
    let Value::Object(enums) = scope.get("__enums")? else {
        return None;
    };
    let Value::List(variants) = enums.get(kind)? else {
        return None;
    };
    variants
        .iter()
        .any(|v| v.text().ok().as_deref() == Some(variant))
        .then(|| Value::Text(path.into()))
}

pub(super) fn expression_number(value: &Value) -> Result<f64, String> {
    match value {
        Value::Number(value) if value.is_finite() => Ok(*value),
        _ => Err("Arithmetic expects a finite Number".into()),
    }
}

pub(super) fn eval(
    value: &Value,
    scope: &Properties,
    state: &Properties,
    stack: &mut Vec<String>,
) -> Result<Value, String> {
    eval_mode(value, scope, state, stack, false)
}

pub(super) fn eval_mode(
    value: &Value,
    scope: &Properties,
    state: &Properties,
    stack: &mut Vec<String>,
    allow_missing: bool,
) -> Result<Value, String> {
    if allow_missing && matches!(value, Value::Expr(_)) {
        return optional_result(eval(value, scope, state, stack));
    }
    let result = match value {
        Value::Expr(path) => {
            let (negative, path) = path
                .strip_prefix('!')
                .map_or((false, path.as_str()), |p| (true, p));
            let result = if let Some(key) = path.strip_prefix("props.") {
                let key = if key == "font.size" { "fontSize" } else { key };
                if stack.iter().any(|v| v == key) {
                    return Err(format!("Cyclic property {key}"));
                }
                let v = lookup(scope, key).ok_or_else(|| format!("Unknown property {path}"))?;
                stack.push(key.into());
                let result = eval(&v, scope, state, stack);
                stack.pop();
                result?
            } else if let Some(key) = path.strip_prefix("tokens.") {
                match scope.get("__tokens") {
                    Some(Value::Object(tokens)) => {
                        lookup(tokens, key).ok_or_else(|| format!("Unknown token {path}"))?
                    }
                    _ => return Err(format!("Unknown token {path}")),
                }
            } else if let Some(key) = path.strip_prefix("i18n.") {
                match scope.get("__i18n") {
                    Some(Value::Object(i18n)) => {
                        lookup(i18n, key).ok_or_else(|| format!("Unknown i18n key {path}"))?
                    }
                    _ => return Err(format!("Unknown i18n key {path}")),
                }
            } else if let Some(key) = path.strip_prefix("state.") {
                lookup(state, key).ok_or_else(|| format!("Missing state.{key}"))?
            } else if path == "null" {
                Value::Null
            } else if let Some(value) = enum_value(path, scope)
                .or_else(|| lookup(state, path))
                .or_else(|| match scope.get("__lets") {
                    Some(Value::Object(lets)) => lookup(lets, path),
                    _ => None,
                })
            {
                if matches!(
                    value,
                    Value::Expression(_)
                        | Value::Expr(_)
                        | Value::Interpolation(_)
                        | Value::Match(..)
                ) {
                    eval(&value, scope, state, stack)?
                } else {
                    value
                }
            } else if path.split_once('.').is_some_and(|(head, _)| {
                matches!(scope.get("__enums"), Some(Value::Object(enums)) if enums.contains_key(head))
            }) {
                return Err(format!("Unknown enum variant {path}"));
            } else if path
                .split_once('.')
                .is_some_and(|(head, _)| state.contains_key(head))
            {
                return Err(format!("Unknown member {path}"));
            } else {
                Value::Expr(path.into())
            };
            if negative {
                Value::Bool(!result.boolean()?)
            } else {
                result
            }
        }
        Value::Expression(expression) => {
            let mut evaluate = |v: &Value, optional| eval_mode(v, scope, state, stack, optional);
            match expression.as_ref() {
                Expression::Unary(operator, argument) => {
                    let argument = evaluate(argument, false)?;
                    match operator.as_str() {
                        "!" => Value::Bool(!argument.boolean()?),
                        "-" => number(-expression_number(&argument)?),
                        "+" => number(expression_number(&argument)?),
                        _ => return Err(format!("Unknown unary operator {operator}")),
                    }
                }
                Expression::Binary(operator, left, right) => {
                    let left = evaluate(left, operator == "??" || allow_missing)?;
                    match operator.as_str() {
                        "&&" if !left.boolean()? => Value::Bool(false),
                        "||" if left.boolean()? => Value::Bool(true),
                        "??" if left != Value::Null => left,
                        _ => {
                            let right = evaluate(right, false)?;
                            match operator.as_str() {
                                "&&" | "||" => Value::Bool(right.boolean()?),
                                "??" => right,
                                "==" | "===" => Value::Bool(left == right),
                                "!=" | "!==" => Value::Bool(left != right),
                                ".." | "..=" => {
                                    let st = expression_number(&left)? as i64;
                                    let en = expression_number(&right)? as i64;
                                    let inclusive = operator == "..=";
                                    let count = if inclusive { en - st + 1 } else { en - st };
                                    if count <= 0 {
                                        Value::List(vec![])
                                    } else if count > 4096 {
                                        return Err("Range exceeds 4096 items".into());
                                    } else if inclusive {
                                        Value::List((st..=en).map(|i| number(i as f64)).collect())
                                    } else {
                                        Value::List((st..en).map(|i| number(i as f64)).collect())
                                    }
                                }
                                "+" if matches!(left, Value::Text(_))
                                    && matches!(right, Value::Text(_)) =>
                                {
                                    Value::Text(left.text()? + &right.text()?)
                                }
                                "+" => {
                                    number(expression_number(&left)? + expression_number(&right)?)
                                }
                                "-" => {
                                    number(expression_number(&left)? - expression_number(&right)?)
                                }
                                "*" => {
                                    number(expression_number(&left)? * expression_number(&right)?)
                                }
                                "/" | "%" if expression_number(&right)? == 0. => {
                                    return Err("Division by zero".into());
                                }
                                "/" => {
                                    number(expression_number(&left)? / expression_number(&right)?)
                                }
                                "%" => {
                                    number(expression_number(&left)? % expression_number(&right)?)
                                }
                                "<" | "<=" | ">" | ">=" => {
                                    let order = match (&left, &right) {
                                        (Value::Text(a), Value::Text(b)) => a.partial_cmp(b),
                                        _ => expression_number(&left)?
                                            .partial_cmp(&expression_number(&right)?),
                                    }
                                    .ok_or("Values cannot be compared")?;
                                    Value::Bool(match operator.as_str() {
                                        "<" => order.is_lt(),
                                        "<=" => order.is_le(),
                                        ">" => order.is_gt(),
                                        _ => order.is_ge(),
                                    })
                                }
                                _ => return Err(format!("Unknown binary operator {operator}")),
                            }
                        }
                    }
                }
                Expression::Conditional(condition, yes, no) => {
                    let condition = evaluate(condition, false)?.boolean()?;
                    evaluate(if condition { yes } else { no }, false)?
                }
                Expression::Member(object, property, optional) => {
                    let optional = *optional || allow_missing;
                    let object = evaluate(object, optional)?;
                    if optional && object == Value::Null {
                        Value::Null
                    } else {
                        let property = evaluate(property, false)?.text()?;
                        if ["__proto__", "prototype", "constructor"].contains(&property.as_str()) {
                            return Err(format!("Unavailable member {property}"));
                        }
                        match object {
                            Value::Object(object) => match object.get(&property).cloned() {
                                Some(value) => value,
                                None if optional => Value::Null,
                                None => return Err(format!("Unknown member {property}")),
                            },
                            Value::List(items) if property == "length" => {
                                number(items.len() as f64)
                            }
                            Value::List(items) => match property
                                .parse::<usize>()
                                .ok()
                                .and_then(|index| items.get(index).cloned())
                            {
                                Some(value) => value,
                                None if optional => Value::Null,
                                None => return Err(format!("Unknown list index {property}")),
                            },
                            Value::Text(value) if property == "length" => {
                                number(value.encode_utf16().count() as f64)
                            }
                            _ => return Err(format!("Cannot read member {property}")),
                        }
                    }
                }
                Expression::Call(name, arguments) => {
                    if name == "coalesce" {
                        if arguments.is_empty() {
                            return Err("coalesce expects at least one argument".into());
                        }
                        let mut found = Value::Null;
                        for arg in arguments {
                            let val = evaluate(arg, true)?;
                            if val != Value::Null {
                                found = val;
                                break;
                            }
                        }
                        found
                    } else {
                        let arguments = arguments
                            .iter()
                            .map(|value| evaluate(value, false))
                            .collect::<Result<Vec<_>, _>>()?;
                        call(name, &arguments)?
                    }
                }
            }
        }
        Value::Interpolation(parts) => {
            let mut out = String::with_capacity(parts.len() * 16);
            for part in parts {
                match eval(part, scope, state, stack)? {
                    Value::Null => {}
                    Value::Text(v) | Value::Expr(v) => out.push_str(&v),
                    Value::Number(v) => {
                        let _ = std::fmt::Write::write_fmt(&mut out, format_args!("{v}"));
                    }
                    Value::Bool(v) => {
                        out.push_str(if v { "true" } else { "false" });
                    }
                    _ => return Err("Expected scalar text".into()),
                }
            }
            Value::Text(out)
        }
        Value::Object(values) => Value::Object(
            values
                .iter()
                .map(|(k, v)| Ok((k.clone(), eval(v, scope, state, stack)?)))
                .collect::<Result<_, String>>()?,
        ),
        Value::List(values) => Value::List(
            values
                .iter()
                .map(|v| eval(v, scope, state, stack))
                .collect::<Result<_, _>>()?,
        ),
        Value::Match(subject, branches) => {
            let subject = eval(subject, scope, state, stack)?;
            let mut selected = None;
            for (pattern, result) in branches {
                if pattern_matches(pattern, &subject, scope, state, stack)? {
                    selected = Some(result);
                    break;
                }
            }
            eval(
                selected.ok_or("No matching branch; add _ => ...")?,
                scope,
                state,
                stack,
            )?
        }
        _ => value.clone(),
    };
    if matches!(&result, Value::Number(n) if !n.is_finite()) {
        return Err("Expression produced a non-finite number".into());
    }
    Ok(result)
}

fn optional_result(result: Result<Value, String>) -> Result<Value, String> {
    match result {
        Err(error)
            if [
                "Missing state.",
                "Unknown property ",
                "Unknown member ",
                "Unknown list index ",
                "Cannot read member ",
            ]
            .iter()
            .any(|prefix| error.starts_with(prefix)) =>
        {
            Ok(Value::Null)
        }
        result => result,
    }
}

fn parse_hex_color(value: &Value) -> Result<(u8, u8, u8, u8), String> {
    let raw = value.text()?;
    let s = raw
        .trim()
        .strip_prefix('#')
        .ok_or("Color function expects a hex color (#rgb, #rrggbb, or #rrggbbaa)")?;
    let hex =
        |slice: &str| u8::from_str_radix(slice, 16).map_err(|_| String::from("Invalid hex color"));
    // Digits are sliced by byte below, so every byte must be one ASCII hex digit: a multi-byte
    // character would be cut in half, and `from_str_radix` alone accepts a leading sign.
    if matches!(s.len(), 3 | 6 | 8) && !s.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("Invalid hex color".into());
    }
    match s.len() {
        3 => {
            let r = hex(&s[0..1])?;
            let g = hex(&s[1..2])?;
            let b = hex(&s[2..3])?;
            Ok((r * 17, g * 17, b * 17, 255))
        }
        6 => Ok((hex(&s[0..2])?, hex(&s[2..4])?, hex(&s[4..6])?, 255)),
        8 => Ok((
            hex(&s[0..2])?,
            hex(&s[2..4])?,
            hex(&s[4..6])?,
            hex(&s[6..8])?,
        )),
        _ => Err("Color function expects #rgb, #rrggbb, or #rrggbbaa".into()),
    }
}

fn format_hex_color(r: u8, g: u8, b: u8, a: u8, as_expr: bool) -> Value {
    let s = if a == 255 {
        format!("#{r:02x}{g:02x}{b:02x}")
    } else {
        format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
    };
    if as_expr {
        Value::Expr(s)
    } else {
        Value::Text(s)
    }
}

fn char_slice_range(s: &str, start_char: usize, end_char: usize) -> &str {
    if start_char >= end_char {
        return "";
    }
    let mut start_byte = s.len();
    let mut end_byte = s.len();
    for (char_idx, (byte_idx, _)) in s.char_indices().enumerate() {
        if char_idx == start_char {
            start_byte = byte_idx;
        }
        if char_idx == end_char {
            end_byte = byte_idx;
            break;
        }
    }
    &s[start_byte..end_byte]
}

pub(super) fn pattern_matches(
    pattern: &Value,
    value: &Value,
    scope: &Properties,
    state: &Properties,
    stack: &mut Vec<String>,
) -> Result<bool, String> {
    if matches!(pattern, Value::Expr(v) if v == "_") {
        return Ok(true);
    }
    if let Value::Object(obj) = pattern {
        if let Some(Value::List(alts)) = obj.get("or") {
            for alt in alts {
                if pattern_matches(alt, value, scope, state, stack)? {
                    return Ok(true);
                }
            }
            return Ok(false);
        }
    }
    if let Value::Expression(expr) = pattern {
        if let Expression::Binary(op, left, right) = expr.as_ref() {
            if op == ".." || op == "..=" {
                let lo = expression_number(&eval(left, scope, state, stack)?)?;
                let hi = expression_number(&eval(right, scope, state, stack)?)?;
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
            if !pattern_matches(pattern, value, scope, state, stack)? {
                return Ok(false);
            }
        }
        return Ok(true);
    }
    Ok(matches(&eval(pattern, scope, state, stack)?, value))
}

fn matches(pattern: &Value, value: &Value) -> bool {
    match (pattern, value) {
        (Value::Expr(v), _) if v == "_" => true,
        (Value::List(a), Value::List(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| matches(a, b))
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
