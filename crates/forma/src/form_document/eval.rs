//! Expression evaluation coordinator for `Document`.

use super::{Expression, Properties, Value, number};

mod builtins;
mod error;
mod lookup;
mod operators;

pub(super) use error::EvalError;
use lookup::eval_expr_path;
pub(super) use lookup::{enum_value, lookup};
use operators::{eval_expression, eval_interpolation, pattern_matches_typed};

pub(super) fn expression_number(value: &Value) -> Result<f64, String> {
    match value {
        Value::Number(value) if value.is_finite() => Ok(*value),
        _ => Err("Arithmetic expects a finite Number".into()),
    }
}

pub(super) fn eval<'a>(
    value: &'a Value,
    scope: &'a Properties,
    state: &'a Properties,
    stack: &mut Vec<&'a str>,
) -> Result<Value, String> {
    eval_typed(value, scope, state, stack).map_err(EvalError::into_message)
}

pub(super) fn eval_typed<'a>(
    value: &'a Value,
    scope: &'a Properties,
    state: &'a Properties,
    stack: &mut Vec<&'a str>,
) -> Result<Value, EvalError> {
    eval_mode_typed(value, scope, state, stack, false)
}

pub(super) fn pattern_matches<'a>(
    pattern: &'a Value,
    value: &Value,
    scope: &'a Properties,
    state: &'a Properties,
    stack: &mut Vec<&'a str>,
) -> Result<bool, String> {
    pattern_matches_typed(pattern, value, scope, state, stack).map_err(EvalError::into_message)
}

pub(super) fn eval_mode_typed<'a>(
    value: &'a Value,
    scope: &'a Properties,
    state: &'a Properties,
    stack: &mut Vec<&'a str>,
    allow_missing: bool,
) -> Result<Value, EvalError> {
    if allow_missing && matches!(value, Value::Expr(_)) {
        return optional_result(eval_typed(value, scope, state, stack));
    }
    let result = match value {
        Value::Expr(path) => eval_expr_path(path, scope, state, stack)?,
        Value::Expression(expression) => {
            eval_expression(expression, scope, state, stack, allow_missing)?
        }
        Value::Interpolation(parts) => eval_interpolation(parts, scope, state, stack)?,
        Value::Object(values) => Value::Object(
            values
                .iter()
                .map(|(k, v)| Ok((k.clone(), eval_typed(v, scope, state, stack)?)))
                .collect::<Result<_, EvalError>>()?,
        ),
        Value::List(values) => Value::List(
            values
                .iter()
                .map(|v| eval_typed(v, scope, state, stack))
                .collect::<Result<_, _>>()?,
        ),
        Value::Match(subject, branches) => {
            let subject = eval_typed(subject, scope, state, stack)?;
            let mut selected = None;
            for (pattern, result) in branches {
                if pattern_matches_typed(pattern, &subject, scope, state, stack)? {
                    selected = Some(result);
                    break;
                }
            }
            eval_typed(
                selected.ok_or(EvalError::NoMatchingBranch)?,
                scope,
                state,
                stack,
            )?
        }
        _ => value.clone(),
    };
    if matches!(&result, Value::Number(n) if !n.is_finite()) {
        return Err(EvalError::NonFiniteNumber);
    }
    Ok(result)
}

fn eval_owned_value<'a>(
    value: &Value,
    scope: &'a Properties,
    state: &'a Properties,
    stack: &mut Vec<&'a str>,
) -> Result<Value, EvalError> {
    // `stack` only stores borrowed `props.<key>` slices from `Value::Expr` nodes;
    // copy current borrowed keys into an inner slice stack when recursing into a
    // freshly cloned `lookup(...)` value whose lifetime ends with this call.
    let mut nested_stack: Vec<&str> = stack.to_vec();
    eval_typed(value, scope, state, &mut nested_stack)
}

fn optional_result(result: Result<Value, EvalError>) -> Result<Value, EvalError> {
    match result {
        Err(error) if error.is_optional_absent() => Ok(Value::Null),
        result => result,
    }
}

pub(super) fn parse_hex_color(value: &Value) -> Result<(u8, u8, u8, u8), String> {
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

pub(super) fn format_hex_color(r: u8, g: u8, b: u8, a: u8, as_expr: bool) -> Value {
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

pub(super) fn char_slice_range(s: &str, start_char: usize, end_char: usize) -> &str {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::form_document::props;

    #[test]
    fn eval_error_preserves_structured_kind_without_string_prefix_matching() {
        let scope = Properties::new();
        let state = props(vec![("user", Value::Object(Properties::new()))]);
        let missing = eval_typed(
            &Value::Expr("state.query".into()),
            &scope,
            &state,
            &mut Vec::new(),
        )
        .unwrap_err();
        assert_eq!(missing, EvalError::MissingState("query".into()));
        assert!(missing.is_missing_state());
        assert_eq!(missing.into_message(), "Missing state.query");

        let div_zero = eval_typed(
            &Value::Expression(Box::new(Expression::Binary(
                "/".into(),
                number(10.),
                number(0.),
            ))),
            &scope,
            &state,
            &mut Vec::new(),
        )
        .unwrap_err();
        assert_eq!(div_zero, EvalError::DivisionByZero);
        assert_eq!(div_zero.into_message(), "Division by zero");

        let unknown_prop = eval_typed(
            &Value::Expr("props.missing".into()),
            &scope,
            &state,
            &mut Vec::new(),
        )
        .unwrap_err();
        assert!(unknown_prop.is_optional_absent());
    }

    #[test]
    fn evaluates_optional_chaining_nullish_coalesce_and_patterns() {
        let scope = props(vec![(
            "__enums",
            Value::Object(props(vec![(
                "Tone",
                Value::List(vec![
                    Value::Text("Primary".into()),
                    Value::Text("Muted".into()),
                ]),
            )])),
        )]);
        let state = props(vec![(
            "items",
            Value::List(vec![Value::Text("alpha".into())]),
        )]);

        let optional_member = Value::Expression(Box::new(Expression::Member(
            Value::Expr("state.missingUser".into()),
            Value::Text("name".into()),
            true,
        )));
        assert_eq!(
            eval(&optional_member, &scope, &state, &mut Vec::new()).unwrap(),
            Value::Null
        );

        let coalesced = Value::Expression(Box::new(Expression::Binary(
            "??".into(),
            optional_member,
            Value::Text("anon".into()),
        )));
        assert_eq!(
            eval(&coalesced, &scope, &state, &mut Vec::new()).unwrap(),
            Value::Text("anon".into())
        );

        let call_coalesce = Value::Expression(Box::new(Expression::Call(
            "coalesce".into(),
            vec![
                Value::Expr("state.missing".into()),
                Value::Expr("items.0".into()),
                Value::Text("fallback".into()),
            ],
        )));
        assert_eq!(
            eval(&call_coalesce, &scope, &state, &mut Vec::new()).unwrap(),
            Value::Text("alpha".into())
        );

        let range_pat = Value::Expression(Box::new(Expression::Binary(
            "..=".into(),
            number(1.),
            number(5.),
        )));
        assert!(pattern_matches(&range_pat, &number(5.), &scope, &state, &mut Vec::new()).unwrap());
        assert!(
            !pattern_matches(&range_pat, &number(6.), &scope, &state, &mut Vec::new()).unwrap()
        );

        let cmp_pat = Value::Object(props(vec![
            ("comparison", Value::Text(">=".into())),
            ("value", number(10.)),
        ]));
        assert!(pattern_matches(&cmp_pat, &number(10.), &scope, &state, &mut Vec::new()).unwrap());
        assert!(!pattern_matches(&cmp_pat, &number(9.), &scope, &state, &mut Vec::new()).unwrap());

        assert_eq!(
            eval(
                &Value::Expr("Tone.Primary".into()),
                &scope,
                &state,
                &mut Vec::new()
            )
            .unwrap(),
            Value::Text("Tone.Primary".into())
        );
    }
}
