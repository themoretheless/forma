//! Builtins that work on text only.

use super::*;

pub(super) fn call(name: &str, arguments: &[Value]) -> Result<Value, String> {
    match name {
        "trim" if arguments.len() == 1 => Ok(Value::Text(arguments[0].text()?.trim().into())),
        "upper" if arguments.len() == 1 => Ok(Value::Text(arguments[0].text()?.to_uppercase())),
        "lower" if arguments.len() == 1 => Ok(Value::Text(arguments[0].text()?.to_lowercase())),
        "capitalize" if arguments.len() == 1 => {
            let s = arguments[0].text()?;
            let mut chars = s.chars();
            let out = match chars.next() {
                Some(first) => {
                    first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase()
                }
                None => String::new(),
            };
            Ok(Value::Text(out))
        }
        "padStart" | "padEnd" if arguments.len() == 2 || arguments.len() == 3 => {
            let s = arguments[0].text()?;
            let target = (expression_number(&arguments[1])?.max(0.) as usize).min(4096);
            let pad = if arguments.len() == 3 {
                arguments[2].text()?
            } else {
                " ".into()
            };
            let cur = s.chars().count();
            if cur >= target || pad.is_empty() {
                return Ok(Value::Text(s));
            }
            let needed = target - cur;
            let fill: String = pad.chars().cycle().take(needed).collect();
            Ok(Value::Text(if name == "padStart" {
                format!("{fill}{s}")
            } else {
                format!("{s}{fill}")
            }))
        }
        "truncate" if arguments.len() == 2 || arguments.len() == 3 => {
            let s = arguments[0].text()?;
            let max_len = expression_number(&arguments[1])?.max(0.) as usize;
            let suffix = if arguments.len() == 3 {
                arguments[2].text()?
            } else {
                "…".into()
            };
            let char_len = s.chars().count();
            if char_len <= max_len {
                Ok(Value::Text(s))
            } else {
                let sfx_len = suffix.chars().count();
                let keep = max_len.saturating_sub(sfx_len);
                let prefix = char_slice_range(&s, 0, keep);
                Ok(Value::Text(format!("{prefix}{suffix}")))
            }
        }
        "repeat" if arguments.len() == 2 => {
            let s = arguments[0].text()?;
            let n = expression_number(&arguments[1])?.max(0.) as usize;
            if s.len().saturating_mul(n) > 65536 {
                return Err("repeat exceeds maximum string length".into());
            }
            Ok(Value::Text(s.repeat(n)))
        }
        "startsWith" if arguments.len() == 2 => Ok(Value::Bool(
            arguments[0].text()?.starts_with(&arguments[1].text()?),
        )),
        "endsWith" if arguments.len() == 2 => Ok(Value::Bool(
            arguments[0].text()?.ends_with(&arguments[1].text()?),
        )),
        "replace" if arguments.len() == 3 => Ok(Value::Text(
            arguments[0]
                .text()?
                .replace(&arguments[1].text()?, &arguments[2].text()?),
        )),
        "split" if arguments.len() == 2 => {
            let s = arguments[0].text()?;
            let sep = arguments[1].text()?;
            Ok(Value::List(
                s.split(&sep).map(|p| Value::Text(p.into())).collect(),
            ))
        }
        "join" if arguments.len() == 1 || arguments.len() == 2 => {
            let Value::List(a) = &arguments[0] else {
                return Err("join expects a list".into());
            };
            let sep = if arguments.len() == 2 {
                arguments[1].text()?
            } else {
                ", ".into()
            };
            let parts = a.iter().map(Value::text).collect::<Result<Vec<_>, _>>()?;
            Ok(Value::Text(parts.join(&sep)))
        }
        _ => Err(unknown(name)),
    }
}
