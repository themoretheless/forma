//! Locale-dependent wording.

use super::*;

pub(super) fn call(name: &str, arguments: &[Value]) -> Result<Value, String> {
    match name {
        "plural" if arguments.len() == 3 || arguments.len() == 4 => {
            let n = expression_number(&arguments[0])?;
            if arguments.len() == 3 {
                let idx = if n.abs() == 1. { 1 } else { 2 };
                Ok(Value::Text(arguments[idx].text()?))
            } else {
                let abs_int = (n.abs().trunc() as i64).abs();
                let mod10 = abs_int % 10;
                let mod100 = abs_int % 100;
                let idx = if n.fract() != 0. {
                    2
                } else if mod10 == 1 && mod100 != 11 {
                    1
                } else if (2..=4).contains(&mod10) && !(12..=14).contains(&mod100) {
                    2
                } else {
                    3
                };
                Ok(Value::Text(arguments[idx].text()?))
            }
        }
        _ => Err(unknown(name)),
    }
}
