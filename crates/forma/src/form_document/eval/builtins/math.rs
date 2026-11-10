//! Numeric builtins: rounding, powers, interpolation and aggregates over number lists.

use super::*;

pub(super) fn call(name: &str, arguments: &[Value]) -> Result<Value, String> {
    let unary = |f: fn(f64) -> f64| -> Result<Value, String> {
        if arguments.len() != 1 {
            return Err(format!("{name} expects one argument"));
        }
        let result = f(expression_number(&arguments[0])?);
        if !result.is_finite() {
            return Err(format!("{name} produced a non-finite number"));
        }
        Ok(number(result))
    };
    match name {
        "abs" => unary(f64::abs),
        "floor" => unary(f64::floor),
        "ceil" => unary(f64::ceil),
        "round" => unary(|v| (v + 0.5).floor()),
        "sqrt" if arguments.len() == 1 => {
            let n = expression_number(&arguments[0])?;
            if n < 0. {
                return Err("sqrt expects a non-negative number".into());
            }
            Ok(number(n.sqrt()))
        }
        "sign" if arguments.len() == 1 => {
            let n = expression_number(&arguments[0])?;
            Ok(number(if n == 0. { 0. } else { n.signum() }))
        }
        "pow" if arguments.len() == 2 => {
            let res = expression_number(&arguments[0])?.powf(expression_number(&arguments[1])?);
            if !res.is_finite() {
                return Err("pow produced a non-finite number".into());
            }
            Ok(number(res))
        }
        "lerp" if arguments.len() == 3 => {
            let a = expression_number(&arguments[0])?;
            let b = expression_number(&arguments[1])?;
            let t = expression_number(&arguments[2])?;
            Ok(number(a + (b - a) * t))
        }
        "remap" if arguments.len() == 5 => {
            let v = expression_number(&arguments[0])?;
            let i0 = expression_number(&arguments[1])?;
            let i1 = expression_number(&arguments[2])?;
            let o0 = expression_number(&arguments[3])?;
            let o1 = expression_number(&arguments[4])?;
            if (i1 - i0).abs() < f64::EPSILON {
                return Err("remap input range cannot be zero".into());
            }
            Ok(number(o0 + ((v - i0) / (i1 - i0)) * (o1 - o0)))
        }
        "avg" if arguments.len() == 1 => {
            let Value::List(items) = &arguments[0] else {
                return Err("avg expects a list of numbers".into());
            };
            if items.is_empty() {
                return Ok(number(0.));
            }
            let mut total = 0.;
            for item in items {
                total += expression_number(item)?;
            }
            Ok(number(total / items.len() as f64))
        }
        "sum" if arguments.len() == 1 => {
            let Value::List(items) = &arguments[0] else {
                return Err("sum expects a list of numbers".into());
            };
            let mut total = 0.;
            for item in items {
                total += expression_number(item)?;
            }
            Ok(number(total))
        }
        "min" | "max" if !arguments.is_empty() => {
            let op = if name == "min" { f64::min } else { f64::max };
            let mut acc = expression_number(&arguments[0])?;
            for arg in &arguments[1..] {
                acc = op(acc, expression_number(arg)?);
            }
            Ok(number(acc))
        }
        "clamp" if arguments.len() == 3 => {
            let (v, min, max) = (
                expression_number(&arguments[0])?,
                expression_number(&arguments[1])?,
                expression_number(&arguments[2])?,
            );
            if min > max {
                return Err("clamp minimum exceeds maximum".into());
            }
            Ok(number(v.clamp(min, max)))
        }
        _ => Err(unknown(name)),
    }
}
