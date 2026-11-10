//! Builtins that accept either text or a list.

use super::*;

pub(super) fn call(name: &str, arguments: &[Value]) -> Result<Value, String> {
    match name {
        "len" if arguments.len() == 1 => Ok(number(match &arguments[0] {
            Value::List(v) => v.len(),
            Value::Text(v) => v.encode_utf16().count(),
            Value::Object(v) => v.len(),
            _ => return Err(format!("{name} expects a list, string, or object")),
        } as f64)),
        "count" if arguments.len() == 1 => {
            let Value::List(v) = &arguments[0] else {
                return Err("count expects a list".into());
            };
            Ok(number(v.len() as f64))
        }
        "isEmpty" if arguments.len() == 1 => Ok(Value::Bool(match &arguments[0] {
            Value::Null => true,
            Value::Text(s) => s.trim().is_empty(),
            Value::List(a) => a.is_empty(),
            Value::Object(o) => o.is_empty(),
            _ => false,
        })),
        "contains" | "includes" if arguments.len() == 2 => match &arguments[0] {
            Value::Text(s) => Ok(Value::Bool(s.contains(&arguments[1].text()?))),
            Value::List(a) => Ok(Value::Bool(a.iter().any(|x| x == &arguments[1]))),
            _ => Err(format!("{name} expects a string or list")),
        },
        "indexOf" if arguments.len() == 2 => match &arguments[0] {
            Value::Text(s) => {
                let sub = arguments[1].text()?;
                let idx = s
                    .find(&sub)
                    .map(|byte_idx| s[..byte_idx].chars().count() as i64)
                    .unwrap_or(-1);
                Ok(number(idx as f64))
            }
            Value::List(a) => {
                let idx = a
                    .iter()
                    .position(|x| x == &arguments[1])
                    .map(|i| i as i64)
                    .unwrap_or(-1);
                Ok(number(idx as f64))
            }
            _ => Err("indexOf expects a string or list".into()),
        },
        "slice" if arguments.len() == 2 || arguments.len() == 3 => {
            let start = expression_number(&arguments[1])? as isize;
            let end = if arguments.len() == 3 {
                Some(expression_number(&arguments[2])? as isize)
            } else {
                None
            };
            let norm = |idx: isize, len: usize| -> usize {
                if idx < 0 {
                    len.saturating_sub(idx.unsigned_abs())
                } else {
                    (idx as usize).min(len)
                }
            };
            match &arguments[0] {
                Value::Text(s) => {
                    let len = s.chars().count();
                    let st = norm(start, len);
                    let en = norm(end.unwrap_or(len as isize), len).max(st);
                    Ok(Value::Text(char_slice_range(s, st, en).to_string()))
                }
                Value::List(a) => {
                    let len = a.len();
                    let st = norm(start, len);
                    let en = norm(end.unwrap_or(len as isize), len).max(st);
                    Ok(Value::List(a[st..en].to_vec()))
                }
                _ => Err("slice expects a string or list".into()),
            }
        }
        "reverse" if arguments.len() == 1 => match &arguments[0] {
            Value::List(a) => {
                let mut v = a.clone();
                v.reverse();
                Ok(Value::List(v))
            }
            Value::Text(s) => Ok(Value::Text(s.chars().rev().collect())),
            _ => Err("reverse expects a list or string".into()),
        },
        _ => Err(unknown(name)),
    }
}
