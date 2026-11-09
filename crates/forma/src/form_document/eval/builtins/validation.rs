//! Value validation rules.

use super::*;

pub(super) fn call(name: &str, arguments: &[Value]) -> Result<Value, String> {
    match name {
        "isValid" if arguments.len() == 2 || arguments.len() == 3 => {
            if arguments.len() == 3 {
                let min = expression_number(&arguments[1])?;
                let max = expression_number(&arguments[2])?;
                let ok = match &arguments[0] {
                    Value::Number(n) => *n >= min && *n <= max,
                    Value::Text(s) => {
                        let len = s.chars().count() as f64;
                        len >= min && len <= max
                    }
                    Value::List(a) => {
                        let len = a.len() as f64;
                        len >= min && len <= max
                    }
                    _ => false,
                };
                Ok(Value::Bool(ok))
            } else {
                let pat = arguments[1].text()?;
                let s = arguments[0].text()?;
                let matched =
                    themoretheless_tokenizer_forma::evaluate::matches_constraint_pattern(&s, &pat)
                        .map_err(|e| e.to_string())?;
                Ok(Value::Bool(matched))
            }
        }
        _ => Err(unknown(name)),
    }
}
