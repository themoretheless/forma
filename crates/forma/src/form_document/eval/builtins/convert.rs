//! Conversions between text, numbers and booleans, and number formatting.

use super::*;

pub(super) fn call(name: &str, arguments: &[Value]) -> Result<Value, String> {
    match name {
        "String" if arguments.len() == 1 => Ok(Value::Text(arguments[0].text()?)),
        "Number" if arguments.len() == 1 => {
            let value = match &arguments[0] {
                Value::Text(v) => v
                    .trim()
                    .parse::<f64>()
                    .map_err(|_| "Number expects a numeric string")?,
                _ => expression_number(&arguments[0])?,
            };
            if !value.is_finite() {
                return Err("Number must be finite".into());
            }
            Ok(number(value))
        }
        "Bool" if arguments.len() == 1 => Ok(Value::Bool(arguments[0].boolean()?)),
        "formatNumber" if arguments.len() == 1 || arguments.len() == 2 => {
            let n = expression_number(&arguments[0])?;
            if arguments.len() == 2 {
                let dec = (expression_number(&arguments[1])?.max(0.) as usize).min(12);
                Ok(Value::Text(format!("{n:.dec$}")))
            } else {
                Ok(Value::Text(if n == 0. {
                    "0".into()
                } else {
                    n.to_string()
                }))
            }
        }
        _ => Err(unknown(name)),
    }
}
