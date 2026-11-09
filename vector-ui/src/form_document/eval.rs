use super::{Expression, Properties, Value, number, props};

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

fn call(name: &str, arguments: &[Value]) -> Result<Value, String> {
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
        "reverse" if arguments.len() == 1 => match &arguments[0] {
            Value::List(a) => {
                let mut v = a.clone();
                v.reverse();
                Ok(Value::List(v))
            }
            Value::Text(s) => Ok(Value::Text(s.chars().rev().collect())),
            _ => Err("reverse expects a list or string".into()),
        },
        "take" if arguments.len() == 2 => {
            let Value::List(a) = &arguments[0] else {
                return Err("take expects a list".into());
            };
            let n = expression_number(&arguments[1])?.max(0.) as usize;
            Ok(Value::List(a.iter().take(n).cloned().collect()))
        }
        "skip" if arguments.len() == 2 => {
            let Value::List(a) = &arguments[0] else {
                return Err("skip expects a list".into());
            };
            let n = expression_number(&arguments[1])?.max(0.) as usize;
            Ok(Value::List(a.iter().skip(n).cloned().collect()))
        }
        "range" if arguments.len() == 2 => {
            let st = expression_number(&arguments[0])? as i64;
            let en = expression_number(&arguments[1])? as i64;
            if en < st {
                return Ok(Value::List(vec![]));
            }
            if en - st > 4096 {
                return Err("range exceeds 4096 items".into());
            }
            Ok(Value::List((st..en).map(|i| number(i as f64)).collect()))
        }
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
        "unique" if arguments.len() == 1 || arguments.len() == 2 => {
            let Value::List(a) = &arguments[0] else {
                return Err("unique expects a list".into());
            };
            let key = if arguments.len() == 2 {
                Some(arguments[1].text()?)
            } else {
                None
            };
            let mut out: Vec<Value> = Vec::with_capacity(a.len());
            for item in a {
                let k = match (&key, item) {
                    (Some(f), Value::Object(o)) => o.get(f).unwrap_or(&Value::Null),
                    (Some(_), _) => &Value::Null,
                    (None, _) => item,
                };
                let exists = out.iter().any(|existing| {
                    let ek = match (&key, existing) {
                        (Some(f), Value::Object(o)) => o.get(f).unwrap_or(&Value::Null),
                        (Some(_), _) => &Value::Null,
                        (None, _) => existing,
                    };
                    ek == k
                });
                if !exists {
                    out.push(item.clone());
                }
            }
            Ok(Value::List(out))
        }
        "find" if arguments.len() == 3 => {
            let Value::List(a) = &arguments[0] else {
                return Err("find expects a list".into());
            };
            let key = arguments[1].text()?;
            let target = &arguments[2];
            let found = a
                .iter()
                .find(|item| matches!(item, Value::Object(o) if o.get(&key) == Some(target)))
                .cloned()
                .unwrap_or(Value::Null);
            Ok(found)
        }
        "compact" if arguments.len() == 1 => {
            let Value::List(a) = &arguments[0] else {
                return Err("compact expects a list".into());
            };
            Ok(Value::List(
                a.iter()
                    .filter(|v| {
                        !matches!(v, Value::Null | Value::Bool(false))
                            && !matches!(v, Value::Text(s) if s.is_empty())
                    })
                    .cloned()
                    .collect(),
            ))
        }
        "flatten" if arguments.len() == 1 => {
            let Value::List(a) = &arguments[0] else {
                return Err("flatten expects a list".into());
            };
            let mut out = Vec::with_capacity(a.len());
            for item in a {
                if let Value::List(sub) = item {
                    out.extend(sub.iter().cloned());
                } else {
                    out.push(item.clone());
                }
            }
            Ok(Value::List(out))
        }
        "chunk" if arguments.len() == 2 => {
            let Value::List(a) = &arguments[0] else {
                return Err("chunk expects a list".into());
            };
            let sz = expression_number(&arguments[1])? as usize;
            if sz == 0 {
                return Err("chunk size must be positive".into());
            }
            Ok(Value::List(
                a.chunks(sz).map(|c| Value::List(c.to_vec())).collect(),
            ))
        }
        "zip" if arguments.len() == 2 => {
            let (Value::List(a), Value::List(b)) = (&arguments[0], &arguments[1]) else {
                return Err("zip expects two lists".into());
            };
            Ok(Value::List(
                a.iter()
                    .zip(b.iter())
                    .map(|(x, y)| Value::List(vec![x.clone(), y.clone()]))
                    .collect(),
            ))
        }
        "first" if arguments.len() == 1 => {
            let Value::List(a) = &arguments[0] else {
                return Err("first expects a list".into());
            };
            Ok(a.first().cloned().unwrap_or(Value::Null))
        }
        "last" if arguments.len() == 1 => {
            let Value::List(a) = &arguments[0] else {
                return Err("last expects a list".into());
            };
            Ok(a.last().cloned().unwrap_or(Value::Null))
        }
        "groupBy" if arguments.len() == 2 => {
            let Value::List(a) = &arguments[0] else {
                return Err("groupBy expects a list".into());
            };
            let key = arguments[1].text()?;
            let mut groups: Vec<(Value, Vec<Value>)> = Vec::new();
            for item in a {
                let k = match item {
                    Value::Object(o) => o.get(&key).cloned().unwrap_or(Value::Null),
                    _ => Value::Null,
                };
                if let Some((_, items)) = groups.iter_mut().find(|(gk, _)| gk == &k) {
                    items.push(item.clone());
                } else {
                    groups.push((k, vec![item.clone()]));
                }
            }
            Ok(Value::List(
                groups
                    .into_iter()
                    .map(|(k, items)| {
                        Value::Object(props(vec![("key", k), ("items", Value::List(items))]))
                    })
                    .collect(),
            ))
        }
        "pluck" if arguments.len() == 2 => {
            let Value::List(a) = &arguments[0] else {
                return Err("pluck expects a list".into());
            };
            let key = arguments[1].text()?;
            Ok(Value::List(
                a.iter()
                    .map(|item| match item {
                        Value::Object(o) => o.get(&key).cloned().unwrap_or(Value::Null),
                        _ => Value::Null,
                    })
                    .collect(),
            ))
        }
        "sortBy" if arguments.len() == 1 || arguments.len() == 2 => {
            let Value::List(a) = &arguments[0] else {
                return Err("sortBy expects a list".into());
            };
            let key = if arguments.len() == 2 {
                Some(arguments[1].text()?)
            } else {
                None
            };
            let mut v = a.clone();
            v.sort_by(|x, y| {
                let kx = match (&key, x) {
                    (Some(f), Value::Object(o)) => o.get(f).unwrap_or(&Value::Null),
                    (Some(_), _) => &Value::Null,
                    (None, _) => x,
                };
                let ky = match (&key, y) {
                    (Some(f), Value::Object(o)) => o.get(f).unwrap_or(&Value::Null),
                    (Some(_), _) => &Value::Null,
                    (None, _) => y,
                };
                match (kx, ky) {
                    (Value::Text(a), Value::Text(b)) => a.cmp(b),
                    (Value::Number(a), Value::Number(b)) => {
                        a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
                    }
                    _ => std::cmp::Ordering::Equal,
                }
            });
            Ok(Value::List(v))
        }
        "any" if arguments.len() == 1 => {
            let Value::List(a) = &arguments[0] else {
                return Err("any expects a list".into());
            };
            let mut res = false;
            for item in a {
                if item.boolean()? {
                    res = true;
                    break;
                }
            }
            Ok(Value::Bool(res))
        }
        "all" if arguments.len() == 1 => {
            let Value::List(a) = &arguments[0] else {
                return Err("all expects a list".into());
            };
            let mut res = true;
            for item in a {
                if !item.boolean()? {
                    res = false;
                    break;
                }
            }
            Ok(Value::Bool(res))
        }
        "keys" if arguments.len() == 1 => {
            let Value::Object(o) = &arguments[0] else {
                return Err("keys expects an object".into());
            };
            Ok(Value::List(
                o.keys().map(|k| Value::Text(k.clone())).collect(),
            ))
        }
        "values" if arguments.len() == 1 => {
            let Value::Object(o) = &arguments[0] else {
                return Err("values expects an object".into());
            };
            Ok(Value::List(o.values().cloned().collect()))
        }
        "entries" if arguments.len() == 1 => {
            let Value::Object(o) = &arguments[0] else {
                return Err("entries expects an object".into());
            };
            Ok(Value::List(
                o.iter()
                    .map(|(k, v)| Value::List(vec![Value::Text(k.clone()), v.clone()]))
                    .collect(),
            ))
        }
        "fromEntries" if arguments.len() == 1 => {
            let Value::List(a) = &arguments[0] else {
                return Err("fromEntries expects a list of pairs".into());
            };
            let mut out = Properties::new();
            for item in a {
                match item {
                    Value::List(pair) if pair.len() == 2 => {
                        out.insert(pair[0].text()?, pair[1].clone());
                    }
                    Value::Object(obj) if obj.contains_key("key") && obj.contains_key("value") => {
                        out.insert(obj["key"].text()?, obj["value"].clone());
                    }
                    _ => return Err("fromEntries expects [key, value] pairs".into()),
                }
            }
            Ok(Value::Object(out))
        }
        "merge" if arguments.len() >= 2 => {
            let mut out = Properties::new();
            for arg in arguments {
                let Value::Object(o) = arg else {
                    return Err("merge expects objects".into());
                };
                out.extend(o.clone());
            }
            Ok(Value::Object(out))
        }
        "pick" | "omit" if arguments.len() == 2 => {
            let Value::Object(o) = &arguments[0] else {
                return Err(format!("{name} expects an object"));
            };
            let Value::List(ks) = &arguments[1] else {
                return Err(format!("{name} expects a list of keys"));
            };
            let mut key_set = std::collections::BTreeSet::new();
            for k in ks {
                key_set.insert(k.text()?);
            }
            let mut out = Properties::new();
            for (k, v) in o {
                let hit = key_set.contains(k);
                if (name == "pick" && hit) || (name == "omit" && !hit) {
                    out.insert(k.clone(), v.clone());
                }
            }
            Ok(Value::Object(out))
        }
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
        "alpha" if arguments.len() == 2 => {
            let (r, g, b, _) = parse_hex_color(&arguments[0])?;
            let a = (expression_number(&arguments[1])?.clamp(0., 1.) * 255.).round() as u8;
            Ok(format_hex_color(
                r,
                g,
                b,
                a,
                matches!(arguments[0], Value::Expr(_)),
            ))
        }
        "lighten" | "darken" if arguments.len() == 2 => {
            let (r, g, b, a) = parse_hex_color(&arguments[0])?;
            let amt = expression_number(&arguments[1])?.clamp(0., 1.);
            let shift = |c: u8| -> u8 {
                if name == "lighten" {
                    (c as f64 + (255. - c as f64) * amt).round() as u8
                } else {
                    (c as f64 * (1. - amt)).round() as u8
                }
            };
            Ok(format_hex_color(
                shift(r),
                shift(g),
                shift(b),
                a,
                matches!(arguments[0], Value::Expr(_)),
            ))
        }
        "saturate" | "desaturate" if arguments.len() == 2 => {
            let (r, g, b, a) = parse_hex_color(&arguments[0])?;
            let amt = expression_number(&arguments[1])?.clamp(0., 1.);
            let (rf, gf, bf) = (r as f64 / 255., g as f64 / 255., b as f64 / 255.);
            let max = rf.max(gf).max(bf);
            let min = rf.min(gf).min(bf);
            let l = (max + min) * 0.5;
            if (max - min).abs() < f64::EPSILON {
                Ok(format_hex_color(
                    r,
                    g,
                    b,
                    a,
                    matches!(arguments[0], Value::Expr(_)),
                ))
            } else {
                let d = max - min;
                let s = if l > 0.5 {
                    d / (2. - max - min)
                } else {
                    d / (max + min)
                };
                let new_s = if name == "saturate" {
                    (s + (1. - s) * amt).clamp(0., 1.)
                } else {
                    (s * (1. - amt)).clamp(0., 1.)
                };
                let factor = if s > 0. { new_s / s } else { 0. };
                let adj = |c: f64| ((l + (c - l) * factor).clamp(0., 1.) * 255.).round() as u8;
                Ok(format_hex_color(
                    adj(rf),
                    adj(gf),
                    adj(bf),
                    a,
                    matches!(arguments[0], Value::Expr(_)),
                ))
            }
        }
        "invert" if arguments.len() == 1 => {
            let (r, g, b, a) = parse_hex_color(&arguments[0])?;
            Ok(format_hex_color(
                255 - r,
                255 - g,
                255 - b,
                a,
                matches!(arguments[0], Value::Expr(_)),
            ))
        }
        "contrast" if arguments.len() == 2 => {
            let (r1, g1, b1, _) = parse_hex_color(&arguments[0])?;
            let (r2, g2, b2, _) = parse_hex_color(&arguments[1])?;
            let lum = |r: u8, g: u8, b: u8| -> f64 {
                let ch = |c: u8| -> f64 {
                    let s = c as f64 / 255.;
                    if s <= 0.03928 {
                        s / 12.92
                    } else {
                        ((s + 0.055) / 1.055).powf(2.4)
                    }
                };
                0.2126 * ch(r) + 0.7152 * ch(g) + 0.0722 * ch(b)
            };
            let l1 = lum(r1, g1, b1);
            let l2 = lum(r2, g2, b2);
            let ratio = (l1.max(l2) + 0.05) / (l1.min(l2) + 0.05);
            Ok(number((ratio * 100.).round() / 100.))
        }
        "mix" if arguments.len() == 2 || arguments.len() == 3 => {
            let (r1, g1, b1, a1) = parse_hex_color(&arguments[0])?;
            let (r2, g2, b2, a2) = parse_hex_color(&arguments[1])?;
            let w = if arguments.len() == 3 {
                expression_number(&arguments[2])?.clamp(0., 1.)
            } else {
                0.5
            };
            let lerp =
                |c1: u8, c2: u8| -> u8 { (c1 as f64 * (1. - w) + c2 as f64 * w).round() as u8 };
            Ok(format_hex_color(
                lerp(r1, r2),
                lerp(g1, g2),
                lerp(b1, b2),
                lerp(a1, a2),
                matches!(arguments[0], Value::Expr(_)) || matches!(arguments[1], Value::Expr(_)),
            ))
        }
        _ => Err(format!("Unknown function or invalid arguments: {name}")),
    }
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
