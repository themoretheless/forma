//! Executable component definitions emitted by the form compiler. No reflection
//! or JS is used in the application: typed model accessors supply named values.
use std::collections::BTreeMap;

pub type Properties = BTreeMap<String, Value>;
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    Text(String),
    Expr(String),
    List(Vec<Value>),
    Node(Box<Node>),
    Match(Box<Value>, Vec<(Value, Value)>),
    Object(Properties),
    Expression(Box<Expression>),
    Interpolation(Vec<Value>),
}
#[derive(Clone, Debug, PartialEq)]
pub enum Expression {
    Unary(String, Value),
    Binary(String, Value, Value),
    Conditional(Value, Value, Value),
    Member(Value, Value, bool),
    Call(String, Vec<Value>),
}
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub kind: String,
    pub props: Properties,
    pub children: Vec<Node>,
    pub action: Option<String>,
}
#[derive(Clone, Debug)]
pub struct Definition {
    pub defaults: Properties,
    pub nodes: Vec<Node>,
}
#[derive(Clone, Debug)]
pub struct Image {
    pub width: f64,
    pub height: f64,
    pub shapes: Vec<(Vec<[f64; 2]>, String)>,
}
#[derive(Clone, Debug)]
pub struct Document {
    pub name: String,
    pub root: Node,
    pub definitions: BTreeMap<String, Definition>,
    pub fallback: Properties,
    pub images: BTreeMap<String, Image>,
    /// One state scope for each node in the scene, in preorder (including containers).
    pub states: Vec<Properties>,
}
#[derive(Clone, Debug)]
pub struct SceneControl {
    /// The control template's index in the original scene's preorder traversal.
    pub node: usize,
    pub key: String,
    pub state: Properties,
    pub event_args: BTreeMap<String, Vec<Value>>,
}
pub fn props(values: Vec<(&str, Value)>) -> Properties {
    values.into_iter().map(|(k, v)| (k.into(), v)).collect()
}
impl Value {
    pub fn evaluate(&self, props: &Properties, state: &Properties) -> Result<Value, String> {
        eval(self, props, state, &mut Vec::new())
    }
    pub fn number(&self) -> Result<f64, String> {
        let value = match self {
            Self::Number(v) => *v,
            Self::Expr(v) if v.ends_with("px") || v.ends_with("ms") => v[..v.len() - 2]
                .parse()
                .map_err(|_| format!("Invalid number {v}"))?,
            _ => return Err(format!("Expected number, got {self:?}")),
        };
        if !value.is_finite() {
            return Err("Number must be finite".into());
        }
        Ok(value)
    }
    pub fn text(&self) -> Result<String, String> {
        match self {
            Self::Null => Ok(String::new()),
            Self::Text(v) | Self::Expr(v) => Ok(v.clone()),
            Self::Number(v) => Ok(v.to_string()),
            Self::Bool(v) => Ok(v.to_string()),
            _ => Err("Expected scalar text".into()),
        }
    }
    pub fn boolean(&self) -> Result<bool, String> {
        if let Self::Bool(v) = self {
            Ok(*v)
        } else {
            Err("Expected boolean".into())
        }
    }
    fn literal(&self) -> Result<String, String> {
        Ok(match self {
            Self::Text(s) => format!(
                "'{}'",
                s.replace('\\', "\\\\")
                    .replace('\'', "\\'")
                    .replace('\n', "\\n")
                    .replace('\r', "\\r")
                    .replace('\t', "\\t")
            ),
            Self::Expr(s) => s.clone(),
            Self::Number(_) => self.number()?.to_string(),
            Self::Bool(v) => v.to_string(),
            Self::List(v) => v
                .iter()
                .map(Self::literal)
                .collect::<Result<Vec<_>, _>>()?
                .join(" "),
            Self::Node(n) => serialize(n)?,
            Self::Match(..) | Self::Expression(_) | Self::Interpolation(_) => {
                return Err("Unevaluated expression".into())
            }
            Self::Object(_) | Self::Null => return Err("Expected serializable visual value".into()),
        })
    }
}
fn lookup(values: &Properties, path: &str) -> Option<Value> {
    if let Some(value) = values.get(path) {
        return Some(value.clone());
    }
    let (head, tail) = path.split_once('.')?;
    match values.get(head)? {
        Value::Object(object) => lookup(object, tail),
        Value::List(items) if tail == "length" => Some(number(items.len() as f64)),
        Value::List(items) => tail
            .parse::<usize>()
            .ok()
            .and_then(|i| items.get(i).cloned()),
        Value::Text(value) if tail == "length" => Some(number(value.encode_utf16().count() as f64)),
        _ => None,
    }
}
fn enum_value(path: &str, scope: &Properties) -> Option<Value> {
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
fn expression_number(value: &Value) -> Result<f64, String> {
    match value {
        Value::Number(value) if value.is_finite() => Ok(*value),
        _ => Err("Arithmetic expects a finite Number".into()),
    }
}
fn eval(
    value: &Value,
    scope: &Properties,
    state: &Properties,
    stack: &mut Vec<String>,
) -> Result<Value, String> {
    eval_mode(value, scope, state, stack, false)
}
fn eval_mode(
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
                    Some(Value::Object(tokens)) => lookup(tokens, key)
                        .ok_or_else(|| format!("Unknown token {path}"))?,
                    _ => return Err(format!("Unknown token {path}")),
                }
            } else if let Some(key) = path.strip_prefix("i18n.") {
                match scope.get("__i18n") {
                    Some(Value::Object(i18n)) => lookup(i18n, key)
                        .ok_or_else(|| format!("Unknown i18n key {path}"))?,
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
                if matches!(value, Value::Expression(_) | Value::Expr(_) | Value::Interpolation(_) | Value::Match(..)) {
                    eval(&value, scope, state, stack)?
                } else {
                    value
                }
            } else if path.split_once('.').is_some_and(|(head,_)| matches!(scope.get("__enums"),Some(Value::Object(enums)) if enums.contains_key(head))) {
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
                                    return Err("Division by zero".into())
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
        Value::Interpolation(parts) => Value::Text(
            parts
                .iter()
                .map(|part| eval(part, scope, state, stack)?.text())
                .collect::<Result<Vec<_>, _>>()?
                .concat(),
        ),
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
    if matches!(&result,Value::Number(n) if !n.is_finite()) {
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
    let hex = |slice: &str| {
        u8::from_str_radix(slice, 16).map_err(|_| String::from("Invalid hex color"))
    };
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
            let values = arguments
                .iter()
                .map(expression_number)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(number(
                values
                    .into_iter()
                    .reduce(if name == "min" { f64::min } else { f64::max })
                    .unwrap(),
            ))
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
            let pad_chars: Vec<char> = pad.chars().collect();
            let fill: String = (0..needed)
                .map(|i| pad_chars[i % pad_chars.len()])
                .collect();
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
            let chars: Vec<char> = s.chars().collect();
            if chars.len() <= max_len {
                Ok(Value::Text(s))
            } else {
                let sfx_len = suffix.chars().count();
                let keep = max_len.saturating_sub(sfx_len);
                let prefix: String = chars[..keep].iter().collect();
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
                    let chars: Vec<char> = s.chars().collect();
                    let len = chars.len();
                    let st = norm(start, len);
                    let en = norm(end.unwrap_or(len as isize), len).max(st);
                    Ok(Value::Text(chars[st..en].iter().collect()))
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
                Ok(Value::Text(if n == 0. { "0".into() } else { n.to_string() }))
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
                        **v != Value::Null
                            && **v != Value::Bool(false)
                            && **v != Value::Text(String::new())
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
            Ok(Value::List(o.keys().map(|k| Value::Text(k.clone())).collect()))
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
                    Value::Object(obj)
                        if obj.contains_key("key") && obj.contains_key("value") =>
                    {
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
                Ok(Value::Bool(
                    themoretheless_tokenizer_forma::evaluate::matches_constraint_pattern(&s, &pat),
                ))
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
            let lerp = |c1: u8, c2: u8| -> u8 {
                (c1 as f64 * (1. - w) + c2 as f64 * w).round() as u8
            };
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
fn pattern_matches(
    pattern: &Value,
    value: &Value,
    scope: &Properties,
    state: &Properties,
    stack: &mut Vec<String>,
) -> Result<bool, String> {
    if matches!(pattern,Value::Expr(v) if v=="_") {
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
fn property_literal(key: &str, value: &Value) -> Result<String, String> {
    if let Value::Text(text) = value {
        let color = key.to_lowercase().contains("color")
            || [
                "background",
                "hoverBackground",
                "pressedBackground",
                "disabledBackground",
                "hover",
                "pressed",
                "disabled",
                "focus",
            ]
            .contains(&key);
        if color
            && text.starts_with('#')
            && [4, 7, 9].contains(&text.len())
            && text[1..].bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Ok(text.clone());
        }
        if key == "overflow" && ["visible", "hidden"].contains(&text.as_str()) {
            return Ok(text.clone());
        }
    }
    value.literal()
}
fn serialize(node: &Node) -> Result<String, String> {
    let properties = node
        .props
        .iter()
        .filter(|(k, _)| {
            k.as_str() != "key"
                && !k.starts_with("__")
                && !["minWidth", "maxWidth", "minHeight", "maxHeight"].contains(&k.as_str())
        })
        .map(|(k, v)| Ok(format!("{k}: {};", property_literal(k, v)?)))
        .collect::<Result<Vec<_>, String>>()?
        .join(" ");
    let action = node
        .action
        .as_ref()
        .map_or(String::new(), |a| format!("clicked -> {a}();"));
    let children = node
        .children
        .iter()
        .map(serialize)
        .collect::<Result<Vec<_>, _>>()?
        .join(" ");
    Ok(format!(
        "{} {{ {properties} {action} {children} }}",
        node.kind
    ))
}
fn num(p: &Properties, key: &str, default: f64) -> Result<f64, String> {
    p.get(key).map_or(Ok(default), Value::number)
}
fn boolean(p: &Properties, key: &str, default: bool) -> Result<bool, String> {
    p.get(key).map_or(Ok(default), Value::boolean)
}
fn text(p: &Properties, key: &str, default: &str) -> Result<String, String> {
    p.get(key).map_or(Ok(default.into()), Value::text)
}
fn number(v: f64) -> Value {
    Value::Number(v)
}
fn insets(value: Option<&Value>) -> Result<[f64; 4], String> {
    let values = match value {
        None => vec![0.],
        Some(Value::List(v)) => v.iter().map(Value::number).collect::<Result<_, _>>()?,
        Some(v) => vec![v.number()?],
    };
    match values.as_slice() {
        [a] => Ok([*a; 4]),
        [a, b] => Ok([*a, *b, *a, *b]),
        [a, b, c] => Ok([*a, *b, *c, *b]),
        [a, b, c, d] => Ok([*a, *b, *c, *d]),
        _ => Err("Expected 1..4 insets".into()),
    }
}
fn gaps(p: &Properties) -> Result<[f64; 2], String> {
    match p.get("gap") {
        None => Ok([0.; 2]),
        Some(Value::List(v)) if v.len() == 2 => Ok([v[0].number()?, v[1].number()?]),
        Some(v) => Ok([v.number()?; 2]),
    }
}
fn tracks(p: &Properties, key: &str) -> Vec<Value> {
    match p.get(key) {
        None => vec![Value::Expr("*".into())],
        Some(Value::List(v)) => v.clone(),
        Some(v) => vec![v.clone()],
    }
}
fn cell(n: &Node, axis: usize, count: usize) -> Result<Option<usize>, String> {
    let value = if let Some(Value::List(values)) = n.props.get("cell") {
        if values.len() != 2 || n.props.contains_key("row") || n.props.contains_key("column") {
            return Err("Invalid cell".into());
        }
        Some(&values[1 - axis])
    } else {
        n.props.get(if axis == 0 { "column" } else { "row" })
    };
    let Some(value) = value else { return Ok(None) };
    let n = value.number()?;
    if n < 1. || n.fract() != 0. || n > count as f64 {
        return Err("Cell outside grid".into());
    }
    Ok(Some(n as usize - 1))
}
fn constraint(n: &Node, axis: usize, value: f64) -> Result<f64, String> {
    let min = num(
        &n.props,
        if axis == 0 { "minWidth" } else { "minHeight" },
        0.,
    )?;
    let max = num(
        &n.props,
        if axis == 0 { "maxWidth" } else { "maxHeight" },
        f64::MAX,
    )?;
    if min < 0. || max < min {
        return Err("Invalid minimum/maximum size".into());
    }
    if !value.is_finite() || value < 0. {
        return Err("Invalid layout size".into());
    }
    Ok(value.clamp(min, max))
}
fn normalize_layout(n: &Node) -> Result<Node, String> {
    let mut n = n.clone();
    match n.kind.as_str() {
        "Row" | "Column" => {
            let row = n.kind == "Row";
            let axis_key = if row { "width" } else { "height" };
            let tracks = n
                .children
                .iter()
                .map(|c| {
                    c.props
                        .get(axis_key)
                        .cloned()
                        .unwrap_or(Value::Expr("auto".into()))
                })
                .collect();
            n.props.insert(
                if row { "columns" } else { "rows" }.into(),
                Value::List(tracks),
            );
            n.props.insert(
                if row { "rows" } else { "columns" }.into(),
                Value::Expr("*".into()),
            );
            for (i, c) in n.children.iter_mut().enumerate() {
                c.props.remove("cell");
                c.props
                    .insert("column".into(), number(if row { i + 1 } else { 1 } as f64));
                c.props
                    .insert("row".into(), number(if row { 1 } else { i + 1 } as f64));
            }
            n.props
                .insert("__linear_axis".into(), number(if row { 0. } else { 1. }));
            n.kind = "Frame".into();
        }
        "Grid" => {
            let columns = tracks(&n.props, "columns").len().max(1);
            let mut rows = tracks(&n.props, "rows");
            if !n.props.contains_key("rows") {
                rows = vec![Value::Expr("auto".into())];
            }
            let mut occupied = std::collections::BTreeSet::new();
            for c in &n.children {
                let column = cell(c, 0, columns)?;
                let row = cell(c, 1, usize::MAX)?;
                if let (Some(column), Some(row)) = (column, row) {
                    if !occupied.insert((row, column)) {
                        return Err("Grid cell is occupied twice".into());
                    }
                }
            }
            for c in &mut n.children {
                let column = cell(c, 0, columns)?;
                let row = cell(c, 1, usize::MAX)?;
                let (row, column) = match (row, column) {
                    (Some(row), Some(column)) => (row, column),
                    (Some(row), None) => (
                        row,
                        (0..columns)
                            .find(|column| !occupied.contains(&(row, *column)))
                            .ok_or("Grid row is full")?,
                    ),
                    (None, Some(column)) => (
                        (0..)
                            .find(|row| !occupied.contains(&(*row, column)))
                            .unwrap(),
                        column,
                    ),
                    (None, None) => (0..)
                        .map(|i| (i / columns, i % columns))
                        .find(|position| !occupied.contains(position))
                        .unwrap(),
                };
                occupied.insert((row, column));
                while rows.len() <= row {
                    rows.push(Value::Expr("auto".into()));
                }
                c.props.remove("cell");
                c.props.insert("row".into(), number((row + 1) as f64));
                c.props.insert("column".into(), number((column + 1) as f64));
            }
            n.props.insert("rows".into(), Value::List(rows));
            n.kind = "Frame".into();
        }
        "Stack" => {
            n.kind = "Frame".into();
            n.props.insert("columns".into(), Value::Expr("*".into()));
            n.props.insert("rows".into(), Value::Expr("*".into()));
        }
        _ => {}
    }
    Ok(n)
}
fn natural(n: &Node, axis: usize) -> Result<f64, String> {
    let key = if axis == 0 { "width" } else { "height" };
    if let Some(value) = n.props.get(key) {
        if let Ok(value) = value.number() {
            return constraint(n, axis, value);
        }
    }
    let n = normalize_layout(n)?;
    let measured = match n.kind.as_str() {
        "Text" => {
            let fs = num(&n.props, "fontSize", 16.)?;
            let text = text(&n.props, "text", "")?;
            let lines: Vec<_> = text.split('\n').collect();
            if axis == 1 && lines.len() > 1 {
                lines.len() as f64 * fs * 1.5
            } else {
                lines
                    .iter()
                    .map(|line| crate::text::measure_line(line, fs as f32)[axis] as f64)
                    .fold(0., f64::max)
            }
        }
        "TextInput" => {
            if axis == 0 {
                80.
            } else {
                num(&n.props, "fontSize", 15.)? * 1.5
            }
        }
        "Image" => 20.,
        "Rectangle" => n
            .children
            .iter()
            .map(|c| natural(c, axis))
            .try_fold(0.0_f64, |a, b| b.map(|b| a.max(b)))?,
        "Frame" | "Scroll" => {
            let ts = tracks(&n.props, if axis == 0 { "columns" } else { "rows" });
            let pad = insets(n.props.get("padding"))?;
            let mut total = pad[1 - axis]
                + pad[3 - axis]
                + gaps(&n.props)?[1 - axis] * ts.len().saturating_sub(1) as f64;
            for (i, t) in ts.iter().enumerate() {
                total += if let Ok(v) = t.number() {
                    v
                } else {
                    let mut size: f64 = 0.;
                    for c in &n.children {
                        if ts.len() == 1 || cell(c, axis, ts.len())? == Some(i) {
                            size = size.max(natural(c, axis)?);
                        }
                    }
                    size
                };
            }
            total
        }
        _ => return Err(format!("Unknown visual {}", n.kind)),
    };
    constraint(&n, axis, measured)
}
fn dimension(n: &Node, axis: usize, available: f64) -> Result<f64, String> {
    let key = if axis == 0 { "width" } else { "height" };
    let value = match n.props.get(key) {
        None => available,
        Some(Value::Expr(v) | Value::Text(v)) if ["auto", "content", "-"].contains(&v.as_str()) => {
            natural(n, axis)?
        }
        Some(Value::Expr(v) | Value::Text(v)) if v.ends_with('*') => available,
        Some(Value::Expr(v) | Value::Text(v)) if v.ends_with('%') => {
            available
                * v[..v.len() - 1]
                    .parse::<f64>()
                    .map_err(|_| "Invalid percentage")?
                / 100.
        }
        Some(v) => v.number()?,
    };
    constraint(n, axis, value)
}
fn sizes(n: &Node, axis: usize, available: f64) -> Result<Vec<f64>, String> {
    let ts = tracks(&n.props, if axis == 0 { "columns" } else { "rows" });
    let mut values = Vec::new();
    let mut weights = Vec::new();
    for (i, t) in ts.iter().enumerate() {
        let mut weight = 0.;
        let size = match t {
            Value::Number(v) => *v,
            Value::Expr(s) | Value::Text(s) if ["-", "content", "auto"].contains(&s.as_str()) => {
                let mut size: f64 = 0.;
                for c in &n.children {
                    if ts.len() == 1 || cell(c, axis, ts.len())? == Some(i) {
                        size = size.max(natural(c, axis)?);
                    }
                }
                size
            }
            Value::Expr(s) | Value::Text(s) if s.ends_with('*') => {
                weight = if s == "*" {
                    1.
                } else {
                    s[..s.len() - 1]
                        .parse()
                        .map_err(|_| "Invalid grid weight")?
                };
                if weight <= 0. {
                    return Err("Grid weight must be positive".into());
                }
                0.
            }
            Value::Expr(s) | Value::Text(s) if s.ends_with('%') => {
                available
                    * s[..s.len() - 1]
                        .parse::<f64>()
                        .map_err(|_| "Invalid percentage")?
                    / 100.
            }
            _ => return Err("Invalid grid track".into()),
        };
        if !size.is_finite() || size < 0. {
            return Err("Invalid grid size".into());
        }
        values.push(size);
        weights.push(weight);
    }
    let mut remaining = (available
        - gaps(&n.props)?[1 - axis] * ts.len().saturating_sub(1) as f64
        - values.iter().sum::<f64>())
    .max(0.);
    let mut pending = (0..weights.len())
        .filter(|i| weights[*i] > 0.)
        .collect::<Vec<_>>();
    while !pending.is_empty() {
        let total = pending.iter().map(|i| weights[*i]).sum::<f64>();
        let mut frozen = Vec::new();
        for &index in &pending {
            let share = remaining * weights[index] / total;
            let mut min = 0.0_f64;
            let mut max = f64::MAX;
            if n.props.get("__linear_axis") == Some(&number(axis as f64)) {
                if let Some(child) = n.children.get(index) {
                    min = num(
                        &child.props,
                        if axis == 0 { "minWidth" } else { "minHeight" },
                        0.,
                    )?;
                    max = num(
                        &child.props,
                        if axis == 0 { "maxWidth" } else { "maxHeight" },
                        f64::MAX,
                    )?;
                    if min < 0. || max < min {
                        return Err("Invalid minimum/maximum size".into());
                    }
                }
            }
            if share < min || share > max {
                frozen.push((index, share.clamp(min, max), share.clamp(min, max) - share));
            }
        }
        if frozen.is_empty() {
            for index in pending {
                values[index] = remaining * weights[index] / total;
            }
            break;
        }
        let violation = frozen.iter().map(|(_, _, delta)| delta).sum::<f64>();
        for (index, value, delta) in frozen {
            if violation > 0. && delta < 0. || violation < 0. && delta > 0. {
                continue;
            }
            values[index] = value;
            remaining = (remaining - value).max(0.);
            pending.retain(|i| *i != index);
        }
    }
    Ok(values)
}
fn select_groups(
    groups: Option<&Value>,
    scope: &Properties,
    state: &Properties,
) -> Result<Properties, String> {
    let mut selected = Properties::new();
    if let Some(Value::List(groups)) = groups {
        for group in groups {
            let Value::Match(subject, branches) = group else {
                return Err("Invalid grouped match".into());
            };
            let subject = subject.evaluate(scope, state)?;
            let mut branch = None;
            for (pattern, value) in branches {
                if pattern_matches(pattern, &subject, scope, state, &mut Vec::new())? {
                    branch = Some(value);
                    break;
                }
            }
            let Value::Object(branch) = branch.ok_or("No matching branch; add _ => ...")? else {
                return Err("Grouped match requires property blocks".into());
            };
            for (key, value) in branch {
                if selected.insert(key.clone(), value.clone()).is_some() {
                    return Err(format!("Multiple match groups set {key}"));
                }
            }
        }
    }
    Ok(selected)
}
fn property_values(n: &Node, scope: &Properties, state: &Properties) -> Result<Properties, String> {
    let mut result = Properties::new();
    if let Some(Value::List(styles)) = n.props.get("__styles") {
        let Some(Value::Object(style_defs)) = scope.get("__styles") else {
            return Err("Unknown applied style".into());
        };
        for style_name in styles {
            let name = style_name.text()?;
            let Some(Value::Object(style_props)) = style_defs.get(&name) else {
                return Err(format!("Unknown applied style {name}"));
            };
            result.extend(style_props.clone());
        }
    }
    if let Some(Value::List(forward)) = n.props.get("__forward") {
        for name in forward {
            let name = name.text()?;
            if !scope.contains_key(&name) {
                return Err(format!("Unknown forwarded property props.{name}"));
            }
            result.insert(name.clone(), Value::Expr(format!("props.{name}")));
        }
    }
    result.extend(select_groups(n.props.get("__matches"), scope, state)?);
    result.extend(
        n.props
            .iter()
            .filter(|(key, _)| !["__matches", "__forward", "__scope", "__styles"].contains(&key.as_str()))
            .map(|(key, value)| (key.clone(), value.clone())),
    );
    Ok(result)
}
fn properties(n: &Node, scope: &Properties, state: &Properties) -> Result<Properties, String> {
    property_values(n, scope, state)?
        .into_iter()
        .map(|(key, value)| Ok((key, value.evaluate(scope, state)?)))
        .collect()
}
fn valid_unit(value: &str, units: &[&str]) -> bool {
    units.iter().any(|unit| {
        value.strip_suffix(unit).is_some_and(|raw| {
            !raw.is_empty()
                && raw.bytes().all(|b| b.is_ascii_digit() || b == b'.')
                && raw.parse::<f64>().is_ok_and(|v| v.is_finite() && v >= 0.)
        })
    })
}
fn valid_signed_unit(value: &str, units: &[&str]) -> bool {
    let value = value
        .strip_prefix('-')
        .or_else(|| value.strip_prefix('+'))
        .unwrap_or(value);
    valid_unit(value, units)
}
fn split_top_level(s: &str, sep: u8) -> Option<Vec<&str>> {
    let mut depth = 0i32;
    let mut parts = Vec::new();
    let mut last = 0usize;
    let bytes = s.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'<' {
            depth += 1;
        } else if b == b'>' {
            depth -= 1;
        } else if b == sep && depth == 0 {
            parts.push(s[last..i].trim());
            last = i + 1;
        }
    }
    if parts.is_empty() {
        None
    } else {
        parts.push(s[last..].trim());
        Some(parts)
    }
}
fn validate_contract_type_name(ty: &str, enums: Option<&Value>) -> Result<(), String> {
    let ty = ty.trim();
    if let Some(parts) = split_top_level(ty, b'|') {
        for p in parts {
            validate_contract_type_name(p, enums)?;
        }
        return Ok(());
    }
    let name = ty.strip_suffix('?').unwrap_or(ty).trim();
    if let Some(inner) = name.strip_prefix("List<").and_then(|s| s.strip_suffix('>')) {
        return validate_contract_type_name(inner, enums);
    }
    if let Some(inner) = name.strip_prefix("Map<").and_then(|s| s.strip_suffix('>')) {
        return validate_contract_type_name(inner, enums);
    }
    if let Some(inner) = name
        .strip_prefix("Record<")
        .and_then(|s| s.strip_suffix('>'))
    {
        let Some(parts) = split_top_level(inner, b',') else {
            return Err(format!("Unknown property type {ty}"));
        };
        if parts.len() != 2 {
            return Err(format!("Unknown property type {ty}"));
        }
        validate_contract_type_name(parts[0], enums)?;
        return validate_contract_type_name(parts[1], enums);
    }
    if [
        "String", "Bool", "Boolean", "Number", "Int", "Float", "Color", "Length", "Duration",
        "Angle", "Asset", "List", "Object", "Map", "Any",
    ]
    .contains(&name)
    {
        return Ok(());
    }
    if let Some(Value::Object(enums)) = enums {
        if enums.contains_key(name) {
            return Ok(());
        }
    }
    Err(format!("Unknown property type {ty}"))
}
fn matches_contract_type(value: &Value, ty: &str, enums: Option<&Value>) -> bool {
    let ty = ty.trim();
    if let Some(parts) = split_top_level(ty, b'|') {
        return parts
            .into_iter()
            .any(|p| matches_contract_type(value, p, enums));
    }
    if let Some(ty) = ty.strip_suffix('?') {
        return value == &Value::Null || matches_contract_type(value, ty.trim(), enums);
    }
    if let Some(inner) = ty.strip_prefix("List<").and_then(|s| s.strip_suffix('>')) {
        return matches!(value, Value::List(items) if items.iter().all(|v| matches_contract_type(v, inner, enums)));
    }
    if let Some(inner) = ty.strip_prefix("Map<").and_then(|s| s.strip_suffix('>')) {
        return matches!(value, Value::Object(obj) if obj.values().all(|v| matches_contract_type(v, inner, enums)));
    }
    if let Some(inner) = ty
        .strip_prefix("Record<")
        .and_then(|s| s.strip_suffix('>'))
    {
        let Some(parts) = split_top_level(inner, b',') else {
            return false;
        };
        if parts.len() != 2 {
            return false;
        }
        let val_ty = parts[1];
        return matches!(value, Value::Object(obj) if obj.values().all(|v| matches_contract_type(v, val_ty, enums)));
    }
    match ty {
        "Any" => true,
        "String" | "Asset" => matches!(value, Value::Text(_)),
        "Color" => value.text().ok().is_some_and(|v| {
            v.starts_with('#')
                && [4, 7, 9].contains(&v.len())
                && v[1..].bytes().all(|b| b.is_ascii_hexdigit())
        }),
        "Bool" | "Boolean" => matches!(value, Value::Bool(_)),
        "Number" | "Float" => matches!(value, Value::Number(n) if n.is_finite()),
        "Int" => {
            matches!(value, Value::Number(n) if n.is_finite() && n.fract() == 0. && n.abs() <= 9_007_199_254_740_991.)
        }
        "Length" => {
            matches!(value, Value::Number(n) if n.is_finite() && *n >= 0.)
                || matches!(value, Value::Text(v) | Value::Expr(v) if valid_unit(v, &["px", "%", "rem", "em", "vw", "vh", "ch", "fr"]))
        }
        "Duration" => {
            matches!(value, Value::Text(v) | Value::Expr(v) if valid_unit(v, &["ms", "s"]))
        }
        "Angle" => {
            matches!(value, Value::Number(n) if n.is_finite())
                || matches!(value, Value::Text(v) | Value::Expr(v) if valid_signed_unit(v, &["deg", "rad", "turn"]))
        }
        "List" => matches!(value, Value::List(_)),
        "Object" | "Map" => matches!(value, Value::Object(_)),
        _ => {
            let Some(Value::Object(enums_map)) = enums else {
                return false;
            };
            match enums_map.get(ty) {
                Some(Value::List(variants)) => variants.iter().any(|v| {
                    v.text()
                        .ok()
                        .map(|variant| Value::Text(format!("{ty}.{variant}")))
                        == Some(value.clone())
                }),
                Some(Value::Object(fields)) => {
                    let Value::Object(obj) = value else {
                        return false;
                    };
                    fields.iter().all(|(field, fty)| {
                        let Ok(fty) = fty.text() else {
                            return false;
                        };
                        match obj.get(field) {
                            Some(v) => matches_contract_type(v, &fty, enums),
                            None => fty.ends_with('?'),
                        }
                    })
                }
                _ => false,
            }
        }
    }
}
fn check_value_constraints(value: &Value, constraints: &Properties, label: &str) -> Result<(), String> {
    if value == &Value::Null {
        return Ok(());
    }
    if let Some(Value::List(range)) = constraints.get("range") {
        if range.len() == 2 {
            let empty = Properties::new();
            let min = expression_number(&range[0].evaluate(&empty, &empty)?)?;
            let max = expression_number(&range[1].evaluate(&empty, &empty)?)?;
            match value {
                Value::Number(n) if *n < min || *n > max => {
                    return Err(format!(
                        "Property {label} value {n} out of range {min}..={max}"
                    ));
                }
                Value::Text(s) => {
                    let len = s.chars().count() as f64;
                    if len < min || len > max {
                        return Err(format!(
                            "Property {label} length {len} out of range {min}..={max}"
                        ));
                    }
                }
                Value::List(a) => {
                    let len = a.len() as f64;
                    if len < min || len > max {
                        return Err(format!(
                            "Property {label} length {len} out of range {min}..={max}"
                        ));
                    }
                }
                _ => {}
            }
        }
    }
    if let Some(pat) = constraints.get("pattern") {
        let pat = pat.text()?;
        let s = value.text()?;
        if !themoretheless_tokenizer_forma::evaluate::matches_constraint_pattern(&s, &pat) {
            return Err(format!(
                "Property {label} does not match pattern '{pat}'"
            ));
        }
    }
    Ok(())
}
fn check_contracts(
    kind: &str,
    contracts: Option<&Value>,
    enums: Option<&Value>,
    values: &Properties,
) -> Result<(), String> {
    let Some(Value::Object(contracts)) = contracts else {
        return Ok(());
    };
    for (name, contract) in contracts {
        let Value::Object(contract) = contract else {
            return Err("Invalid property contract".into());
        };
        let required = boolean(contract, "required", false)?;
        let ty = text(contract, "type", "")?;
        if !ty.is_empty() {
            validate_contract_type_name(&ty, enums)?;
        }
        let Some(value) = values.get(name) else {
            if required {
                return Err(format!("Missing required property {kind}.{name}"));
            }
            continue;
        };
        if ty.is_empty() {
            continue;
        }
        if !matches_contract_type(value, &ty, enums) {
            return Err(format!(
                "Property {kind}.{name} expects {ty}, got {value:?}"
            ));
        }
        if let Some(Value::Object(constraints)) = contract.get("constraints") {
            check_value_constraints(value, constraints, &format!("{kind}.{name}"))?;
        }
    }
    Ok(())
}
fn key_part(value: &Value) -> Result<String, String> {
    let (tag, text) = match value {
        Value::Text(v) => ("s", v.clone()),
        Value::Number(v) if v.is_finite() => ("n", v.to_string()),
        _ => return Err("Collection key must be a string or finite number".into()),
    };
    Ok(format!("{tag}{}:{text}", text.len()))
}
fn group_declares(groups: Option<&Value>, key: &str) -> bool {
    matches!(groups,Some(Value::List(groups)) if groups.iter().any(|group| matches!(group,Value::Match(_,branches) if branches.iter().any(|(_,values)| matches!(values,Value::Object(values) if values.contains_key(key))))))
}
fn is_primitive(kind: &str) -> bool {
    (is_layout(kind) && kind != "Scroll")
        || [
            "Text",
            "TextInput",
            "Image",
            "Rectangle",
            "ContentPresenter",
            "Brush",
            "Border",
            "Reveal",
            "PointerArea",
            "ContentText",
            "ContentShape",
            "ContentClip",
            "ContentClipEnd",
        ]
        .contains(&kind)
}
fn is_layout(kind: &str) -> bool {
    ["Frame", "Row", "Column", "Grid", "Stack", "Scroll"].contains(&kind)
}
impl Document {
    fn expand_value(
        &self,
        value: &Value,
        props: &Properties,
        state: &Properties,
        parents: &[String],
        depth: usize,
    ) -> Result<Value, String> {
        let value = eval(value, props, state, &mut Vec::new())?;
        if let Value::Node(node) = value {
            Ok(Value::Node(Box::new(self.expand(
                &node,
                props,
                state,
                parents,
                depth + 1,
            )?)))
        } else {
            Ok(value)
        }
    }
    fn expand_sequence(
        &self,
        nodes: &[Node],
        props: &Properties,
        state: &Properties,
        parents: &[String],
        depth: usize,
    ) -> Result<Vec<Node>, String> {
        if depth > 64 {
            return Err("Visual composition depth exceeded".into());
        }
        let mut result = Vec::new();
        for node in nodes {
            match node.kind.as_str() {
                "If" => {
                    let condition = node
                        .props
                        .get("condition")
                        .ok_or("If requires a condition")?
                        .evaluate(props, state)?
                        .boolean()?;
                    let children = if condition {
                        node.children
                            .iter()
                            .filter(|n| n.kind != "Else")
                            .cloned()
                            .collect()
                    } else {
                        node.children
                            .iter()
                            .find(|n| n.kind == "Else")
                            .map_or(Vec::new(), |n| n.children.clone())
                    };
                    result.extend(self.expand_sequence(
                        &children,
                        props,
                        state,
                        parents,
                        depth + 1,
                    )?);
                }
                "For" => {
                    let Value::List(items) = node
                        .props
                        .get("collection")
                        .ok_or("For requires a collection")?
                        .evaluate(props, state)?
                    else {
                        return Err("For expects a list".into());
                    };
                    let item = text(&node.props, "item", "item")?;
                    let mut keys = std::collections::BTreeSet::new();
                    if items.is_empty() {
                        if let Some(empty) = node.children.iter().find(|n| n.kind == "Empty") {
                            result.extend(self.expand_sequence(
                                &empty.children,
                                props,
                                state,
                                parents,
                                depth + 1,
                            )?);
                        }
                    }
                    for (index, value) in items.into_iter().enumerate() {
                        let mut state = state.clone();
                        state.insert(item.clone(), value);
                        state.insert(format!("__index.{item}"), number(index as f64));
                        let key = key_part(
                            &node
                                .props
                                .get("key")
                                .ok_or("For requires a key")?
                                .evaluate(props, &state)?,
                        )?;
                        if !keys.insert(key) {
                            return Err("Duplicate collection key".into());
                        }
                        let children = node
                            .children
                            .iter()
                            .filter(|n| n.kind != "Empty")
                            .cloned()
                            .collect::<Vec<_>>();
                        result.extend(self.expand_sequence(
                            &children,
                            props,
                            &state,
                            parents,
                            depth + 1,
                        )?);
                    }
                }
                "Else" | "Empty" => {}
                _ => result.push(self.expand(node, props, state, parents, depth + 1)?),
            }
        }
        Ok(result)
    }
    fn expand(
        &self,
        n: &Node,
        props: &Properties,
        state: &Properties,
        parents: &[String],
        depth: usize,
    ) -> Result<Node, String> {
        if depth > 64 {
            return Err("Visual composition depth exceeded".into());
        }
        if n.kind == "ContentPresenter" {
            let value = n.props.get("content").cloned().unwrap_or_else(|| {
                if n.children.len() == 1 {
                    Value::Node(Box::new(n.children[0].clone()))
                } else {
                    Value::Node(Box::new(Node {
                        kind: "Frame".into(),
                        props: Properties::new(),
                        children: n.children.clone(),
                        action: None,
                    }))
                }
            });
            return match self.expand_value(&value, props, state, parents, depth)? {
                Value::Node(n) => Ok(*n),
                _ => Err("ContentPresenter requires a node".into()),
            };
        }
        let mut node = n.clone();
        node.props = properties(n, props, state)?
            .into_iter()
            .map(|(k, v)| Ok((k, self.expand_value(&v, props, state, parents, depth)?)))
            .collect::<Result<_, String>>()?;
        node.children = self.expand_sequence(&n.children, props, state, parents, depth + 1)?;
        // Scene definitions can wrap primitives such as Text and TextInput.
        // Inside their visual templates the same names retain primitive meaning.
        if !is_primitive(&n.kind) && self.definitions.contains_key(&n.kind) {
            return Ok(self.instantiate(&node, false, state, parents)?.0);
        }
        Ok(node)
    }
    fn instantiate(
        &self,
        instance: &Node,
        top: bool,
        state: &Properties,
        parents: &[String],
    ) -> Result<(Node, Properties), String> {
        if parents.len() >= 32 || parents.contains(&instance.kind) {
            return Err(format!("Component cycle {}", instance.kind));
        }
        let definition = self
            .definitions
            .get(&instance.kind)
            .ok_or_else(|| format!("Missing component {}", instance.kind))?;
        for key in instance.props.keys() {
            if !definition.defaults.contains_key(key)
                && !group_declares(definition.defaults.get("__matches"), key)
                && !matches!(definition.defaults.get("__contracts"),Some(Value::Object(c)) if c.contains_key(key))
                && !key.starts_with("__")
                && !(top && self.fallback.contains_key(key))
                && ![
                    "key",
                    "x",
                    "y",
                    "cell",
                    "row",
                    "column",
                    "width",
                    "height",
                    "font.size",
                    "minWidth",
                    "maxWidth",
                    "minHeight",
                    "maxHeight",
                ]
                .contains(&key.as_str())
            {
                return Err(format!("Unknown property {}.{key}", instance.kind));
            }
        }
        let supplied = definition
            .defaults
            .keys()
            .chain(instance.props.keys())
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        let mut props = if top {
            self.fallback.clone()
        } else {
            Properties::new()
        };
        props.extend(definition.defaults.clone());
        props.extend(instance.props.clone());
        let selected = select_groups(definition.defaults.get("__matches"), &props, state)?;
        props.extend(selected);
        props.extend(instance.props.clone());
        if let Some(value) = instance.props.get("font.size") {
            props.insert("fontSize".into(), value.clone());
        }
        let mut parents = parents.to_vec();
        parents.push(instance.kind.clone());
        let resolved: Properties = props
            .iter()
            .filter(|(k, _)| !k.starts_with("__"))
            .map(|(k, v)| Ok((k.clone(), self.expand_value(v, &props, state, &parents, 0)?)))
            .collect::<Result<_, String>>()?;
        let mut contract_values = resolved.clone();
        if let Some(Value::Object(contracts)) = definition.defaults.get("__contracts") {
            for (key, contract) in contracts {
                if matches!(contract,Value::Object(contract) if contract.get("required")==Some(&Value::Bool(true)))
                    && !supplied.contains(key)
                {
                    contract_values.remove(key);
                }
            }
        }
        check_contracts(
            &instance.kind,
            definition.defaults.get("__contracts"),
            definition.defaults.get("__enums"),
            &contract_values,
        )?;
        let mut nodes = definition.nodes.clone();
        if !instance.children.is_empty() {
            fn supply(nodes: &mut [Node], children: &[Node]) -> bool {
                let mut found = false;
                for node in nodes {
                    if node.kind == "ContentPresenter"
                        && node.props.get("key") == Some(&Value::Text("content".into()))
                    {
                        node.props.insert(
                            "content".into(),
                            Value::Node(Box::new(Node {
                                kind: "Frame".into(),
                                props: Properties::new(),
                                children: children.to_vec(),
                                action: None,
                            })),
                        );
                        found = true;
                    }
                    found |= supply(&mut node.children, children);
                }
                found
            }
            if !supply(&mut nodes, &instance.children) {
                return Err("Child content requires ContentPresenter".into());
            }
        }
        if nodes.len() != 1 {
            return Err("A component requires one root".into());
        }
        let mut root = self.expand(&nodes[0], &props, state, &parents, 0)?;
        if top && root.kind != "Rectangle" {
            return Err("A control requires a Rectangle root".into());
        }
        if !top {
            for key in [
                "key",
                "cell",
                "row",
                "column",
                "width",
                "height",
                "minWidth",
                "maxWidth",
                "minHeight",
                "maxHeight",
            ] {
                if let Some(value) = resolved.get(key) {
                    root.props.insert(key.into(), value.clone());
                }
            }
        }
        Ok((root, resolved))
    }
    fn flatten(&self, n: &Node, bounds: [f64; 4], out: &mut String) -> Result<(), String> {
        let [mut x, mut y, mut w, mut h] = bounds;
        let n = &normalize_layout(n)?;
        let width = dimension(n, 0, w)?;
        let height = dimension(n, 1, h)?;
        x += (w - width) / 2.;
        y += (h - height) / 2.;
        w = width;
        h = height;
        let clip = |out: &mut String, radius: f64| {
            out.push_str(&format!(
                "ContentClip {{ x:{x}; y:{y}; width:{w}; height:{h}; radius:{radius}; }} "
            ))
        };
        match n.kind.as_str() {
            "Frame" => {
                let clipped = boolean(&n.props, "clip", false)?;
                if clipped {
                    clip(out, num(&n.props, "radius", 0.)?);
                }
                let p = insets(n.props.get("padding"))?;
                x += p[3];
                y += p[0];
                w = (w - p[1] - p[3]).max(0.);
                h = (h - p[0] - p[2]).max(0.);
                let cols = sizes(n, 0, w)?;
                let rows = sizes(n, 1, h)?;
                let gap = gaps(&n.props)?;
                for c in &n.children {
                    let ci = cell(c, 0, cols.len())?;
                    let ri = cell(c, 1, rows.len())?;
                    self.flatten(
                        c,
                        [
                            x + ci
                                .map_or(0., |i| cols[..i].iter().sum::<f64>() + i as f64 * gap[1]),
                            y + ri
                                .map_or(0., |i| rows[..i].iter().sum::<f64>() + i as f64 * gap[0]),
                            ci.map_or(w, |i| cols[i]),
                            ri.map_or(h, |i| rows[i]),
                        ],
                        out,
                    )?;
                }
                if clipped {
                    out.push_str("ContentClipEnd {} ");
                }
            }
            "Rectangle" => {
                clip(out, num(&n.props, "radius", 0.)?);
                if let Some(color) = n.props.get("background") {
                    out.push_str(&format!(
                        "ContentShape {{ points:'{x} {y} {} {y} {} {} {x} {}'; color:{}; }} ",
                        x + w,
                        x + w,
                        y + h,
                        y + h,
                        color.literal()?
                    ));
                }
                for c in &n.children {
                    self.flatten(c, [x, y, w, h], out)?;
                }
                out.push_str("ContentClipEnd {} ");
            }
            "TextInput" => {
                out.push_str(&format!("ContentInput {{ x:{x}; y:{y}; width:{w}; height:{h}; value:{}; placeholder:{}; color:{}; placeholderColor:{}; fontSize:{}; multiline:{}; }} ",Value::Text(text(&n.props,"value","")?).literal()?,Value::Text(text(&n.props,"placeholder","")?).literal()?,text(&n.props,"color","#ffffff")?,text(&n.props,"placeholderColor","#999999")?,num(&n.props,"fontSize",15.)?,boolean(&n.props,"multiline",false)?));
            }
            "Text" => {
                let value = text(&n.props, "text", "")?;
                let fs = num(&n.props, "fontSize", num(&n.props, "font.size", 16.)?)?;
                let lines: Vec<_> = value.split('\n').collect();
                if lines.len() > 1 {
                    clip(out, 0.);
                }
                for (i, line) in lines.iter().enumerate() {
                    let (width, height) = if lines.len() > 1 {
                        (
                            crate::text::measure_line(line, fs as f32)[0] as f64,
                            fs * 1.5,
                        )
                    } else {
                        (w, h)
                    };
                    out.push_str(&format!("ContentText {{ x:{x}; y:{}; width:{width}; height:{height}; text:{}; color:{}; fontSize:{fs}; }} ",y+i as f64*fs*1.5,Value::Text((*line).into()).literal()?,text(&n.props,"color","#ffffff")?));
                }
                if lines.len() > 1 {
                    out.push_str("ContentClipEnd {} ");
                }
            }
            "Image" => {
                let source = text(&n.props, "source", "")?;
                let image = self
                    .images
                    .get(&source)
                    .ok_or_else(|| format!("Missing SVG {source}"))?;
                let scale = (w / image.width).min(h / image.height);
                let dx = x + (w - image.width * scale) / 2.;
                let dy = y + (h - image.height * scale) / 2.;
                for (points, color) in &image.shapes {
                    let color = if color == "currentColor" {
                        text(&n.props, "color", "#ffffff")?
                    } else {
                        color.clone()
                    };
                    out.push_str("ContentShape { points:'");
                    for (i, p) in points.iter().enumerate() {
                        if i > 0 {
                            out.push(' ');
                        }
                        let _ = std::fmt::Write::write_fmt(
                            out,
                            format_args!("{} {}", dx + p[0] * scale, dy + p[1] * scale),
                        );
                    }
                    let _ = std::fmt::Write::write_fmt(out, format_args!("'; color:{color}; }} "));
                }
            }
            _ => return Err(format!("Unknown visual {}", n.kind)),
        }
        Ok(())
    }
    fn scene_properties(
        &self,
        node: &Node,
        scope: &Properties,
        state: &Properties,
    ) -> Result<Properties, String> {
        let mut defaults = self.fallback.clone();
        if let Some(definition) = self.definitions.get(&node.kind) {
            defaults.extend(definition.defaults.clone());
        }
        let mut scope = scope.clone();
        for meta_key in ["__enums", "__tokens", "__i18n", "__lets", "__styles"] {
            if let Some(Value::Object(meta_obj)) = defaults.get(meta_key) {
                let mut merged = match scope.remove(meta_key) {
                    Some(Value::Object(v)) => v,
                    _ => Properties::new(),
                };
                merged.extend(meta_obj.clone());
                scope.insert(meta_key.into(), Value::Object(merged));
            }
        }
        let mut result = Properties::new();
        let values = property_values(node, &scope, state)?;
        for (key, value) in &values {
            if key == "__matches" || key == "__forward" || key == "__scope" || key == "__styles" {
                continue;
            }
            match eval(value, &scope, state, &mut Vec::new()) {
                Ok(value) => {
                    result.insert(key.clone(), value);
                }
                Err(error) if error.starts_with("Missing state.") => {
                    if let Some(default) = defaults.get(key) {
                        result.insert(
                            key.clone(),
                            eval(default, &defaults, state, &mut Vec::new())?,
                        );
                    }
                }
                Err(error) => return Err(error),
            }
        }
        if result.get("__range_normalized") == Some(&Value::Bool(true)) {
            if let Some(Value::Number(value)) = result.get("value") {
                result.insert("value".into(), Value::Expr(format!("{}%", value * 100.)));
            }
        }
        Ok(result)
    }
    fn scene_expand(
        &self,
        node: &Node,
        inherited: &Properties,
        prefix: &str,
        inherited_props: &Properties,
        depth: usize,
        budget: &mut usize,
    ) -> Result<Vec<Node>, String> {
        if depth > 64 {
            return Err("Scene composition depth exceeded".into());
        }
        if *budget == 0 {
            return Err("Scene exceeds 16384 nodes".into());
        }
        *budget -= 1;
        let id = num(&node.props, "__node", 0.)? as usize;
        let mut state = inherited.clone();
        if let Some(local) = self.states.get(id) {
            state.extend(local.clone());
        }
        let mut scope = inherited_props.clone();
        if let Some(Value::Object(values)) = node.props.get("__scope") {
            scope.extend(values.clone());
            let selected = select_groups(values.get("__matches"), &scope, &state)?;
            scope.extend(selected);
            let resolved = scope
                .iter()
                .filter(|(key, _)| !key.starts_with("__"))
                .map(|(key, value)| Ok((key.clone(), value.evaluate(&scope, &state)?)))
                .collect::<Result<Properties, String>>()?;
            let mut contract_values = resolved.clone();
            if let Some(Value::Object(contracts)) = values.get("__contracts") {
                for (key, contract) in contracts {
                    if matches!(contract,Value::Object(contract) if contract.get("required")==Some(&Value::Bool(true)))
                        && !values.contains_key(key)
                    {
                        contract_values.remove(key);
                    }
                }
            }
            check_contracts(
                &self.name,
                scope.get("__contracts"),
                scope.get("__enums"),
                &contract_values,
            )?;
            scope.extend(resolved);
        }
        match node.kind.as_str() {
            "If" => {
                let condition = node
                    .props
                    .get("condition")
                    .ok_or("If requires a condition")?
                    .evaluate(&scope, &state)?
                    .boolean()?;
                let children = if condition {
                    node.children
                        .iter()
                        .filter(|n| n.kind != "Else")
                        .cloned()
                        .collect()
                } else {
                    node.children
                        .iter()
                        .find(|n| n.kind == "Else")
                        .map_or(Vec::new(), |n| n.children.clone())
                };
                let mut out = Vec::new();
                for child in children {
                    out.extend(self.scene_expand(
                        &child,
                        &state,
                        prefix,
                        &scope,
                        depth + 1,
                        budget,
                    )?);
                }
                Ok(out)
            }
            "For" => {
                let Value::List(items) = node
                    .props
                    .get("collection")
                    .ok_or("For requires a collection")?
                    .evaluate(&scope, &state)?
                else {
                    return Err("For expects a list".into());
                };
                let item = text(&node.props, "item", "item")?;
                let mut out = Vec::new();
                let mut keys = std::collections::BTreeSet::new();
                if items.is_empty() {
                    if let Some(empty) = node.children.iter().find(|n| n.kind == "Empty") {
                        for child in &empty.children {
                            out.extend(self.scene_expand(
                                child,
                                &state,
                                prefix,
                                &scope,
                                depth + 1,
                                budget,
                            )?);
                        }
                    }
                }
                for (index, value) in items.into_iter().enumerate() {
                    let mut local = state.clone();
                    local.insert(item.clone(), value);
                    local.insert(format!("__index.{item}"), number(index as f64));
                    let key = key_part(
                        &node
                            .props
                            .get("key")
                            .ok_or("For requires a key")?
                            .evaluate(&scope, &local)?,
                    )?;
                    if !keys.insert(key.clone()) {
                        return Err("Duplicate collection key".into());
                    }
                    let prefix = format!("{prefix}@{id}:{key}/");
                    for child in node.children.iter().filter(|n| n.kind != "Empty") {
                        out.extend(self.scene_expand(
                            child,
                            &local,
                            &prefix,
                            &scope,
                            depth + 1,
                            budget,
                        )?);
                    }
                }
                Ok(out)
            }
            "Else" | "Empty" => Ok(Vec::new()),
            _ => {
                let mut output = node.clone();
                output.props = self.scene_properties(node, &scope, &state)?;
                output
                    .props
                    .insert("__state".into(), Value::Object(state.clone()));
                if self.definitions.contains_key(&node.kind) {
                    let explicit = output
                        .props
                        .get("key")
                        .map(Value::text)
                        .transpose()?
                        .unwrap_or_default();
                    let key = if prefix.is_empty() && !explicit.is_empty() {
                        explicit
                    } else {
                        format!("{prefix}@{id}:{}:{explicit}", explicit.len())
                    };
                    output.props.insert("key".into(), Value::Text(key));
                }
                if self.definitions.contains_key(&node.kind) {
                    // Supplied content resolves props in the receiving component's scope.
                    output.children = node.children.clone();
                } else {
                    output.children.clear();
                    for child in &node.children {
                        output.children.extend(self.scene_expand(
                            child,
                            &state,
                            prefix,
                            &scope,
                            depth + 1,
                            budget,
                        )?);
                    }
                }
                Ok(vec![output])
            }
        }
    }
    fn scene_visual(&self, node: &Node) -> Result<Node, String> {
        if is_layout(&node.kind) {
            let mut visual = node.clone();
            visual.children = node
                .children
                .iter()
                .map(|n| self.scene_visual(n))
                .collect::<Result<_, _>>()?;
            normalize_layout(&visual)
        } else {
            let empty = Properties::new();
            let state = match node.props.get("__state") {
                Some(Value::Object(state)) => state,
                _ => &empty,
            };
            self.instantiate(node, true, state, &[])
                .map(|(mut root, p)| {
                    let orig = root.props.clone();
                    root.props.extend(p);
                    root.props.insert("__rect_props".into(), Value::Object(orig));
                    root
                })
        }
    }
    fn position_scene(
        &self,
        node: &Node,
        precomputed_visual: Option<&Node>,
        bounds: [f64; 4],
        instances: &mut Vec<(Node, Option<Node>)>,
        root_legacy: bool,
    ) -> Result<(), String> {
        if !is_layout(&node.kind) {
            let mut node = node.clone();
            let owned_visual;
            let visual = match precomputed_visual {
                Some(v) => v,
                None => {
                    owned_visual = self.scene_visual(&node)?;
                    &owned_visual
                }
            };
            let width = dimension(visual, 0, bounds[2])?;
            let height = dimension(visual, 1, bounds[3])?;
            for (key, value) in [
                ("x", bounds[0] + (bounds[2] - width) / 2.),
                ("y", bounds[1] + (bounds[3] - height) / 2.),
                ("width", width),
                ("height", height),
            ] {
                node.props.insert(key.into(), number(value));
            }
            for key in ["cell", "row", "column"] {
                node.props.remove(key);
            }
            instances.push((node, Some(visual.clone())));
            return Ok(());
        }
        if !root_legacy && boolean(&node.props, "clip", false)? {
            return Err("Nested scene clipping is unsupported; use the root Frame clip".into());
        }
        let mut node = node.clone();
        if root_legacy
            && ["Frame", "Scroll"].contains(&node.kind.as_str())
            && !node.props.contains_key("columns")
            && !node.props.contains_key("rows")
        {
            node.kind = "Column".into();
        }
        let node = normalize_layout(&node)?;
        let owned_visual;
        let visual = match precomputed_visual {
            Some(v) if v.children.len() == node.children.len() => v,
            _ => {
                owned_visual = self.scene_visual(&node)?;
                &owned_visual
            }
        };
        let [mut x, mut y, mut width, mut height] = bounds;
        // A container consumes its allocated cell; its explicit dimensions are resolved by its parent.
        let pad = insets(node.props.get("padding"))?;
        x += pad[3];
        y += pad[0];
        width = (width - pad[1] - pad[3]).max(0.);
        height = (height - pad[0] - pad[2]).max(0.);
        let cols = sizes(visual, 0, width)?;
        let rows = sizes(visual, 1, height)?;
        let gap = gaps(&node.props)?;
        for (child, child_visual) in node.children.iter().zip(&visual.children) {
            let ci = cell(child, 0, cols.len())?;
            let ri = cell(child, 1, rows.len())?;
            let cw = ci.map_or(width, |i| cols[i]);
            let ch = ri.map_or(height, |i| rows[i]);
            let mut cx = x + ci.map_or(0., |i| cols[..i].iter().sum::<f64>() + i as f64 * gap[1]);
            let mut cy = y + ri.map_or(0., |i| rows[..i].iter().sum::<f64>() + i as f64 * gap[0]);
            if is_layout(&child.kind) {
                let w = dimension(child_visual, 0, cw)?;
                let h = dimension(child_visual, 1, ch)?;
                cx += (cw - w) / 2.;
                cy += (ch - h) / 2.;
                self.position_scene(child, Some(child_visual), [cx, cy, w, h], instances, false)?;
            } else {
                self.position_scene(child, Some(child_visual), [cx, cy, cw, ch], instances, false)?;
            }
        }
        Ok(())
    }
    /// Re-evaluate the retained program. The binding owner keeps the last valid scene on error.
    pub fn render(&self) -> Result<(String, String), String> {
        self.render_scene()
            .map(|(source, templates, _)| (source, templates))
    }
    pub fn render_scene(&self) -> Result<(String, String, Vec<SceneControl>), String> {
        fn annotate(node: &mut Node, index: &mut usize, depth: usize) -> Result<(), String> {
            if depth > 64 {
                return Err("Scene composition depth exceeded".into());
            }
            if *index >= 16_384 {
                return Err("Scene exceeds 16384 nodes".into());
            }
            node.props
                .entry("__node".into())
                .or_insert(number(*index as f64));
            *index += 1;
            for child in &mut node.children {
                annotate(child, index, depth + 1)?;
            }
            Ok(())
        }
        let mut program = self.root.clone();
        annotate(&mut program, &mut 0, 0)?;
        let mut expanded = self.scene_expand(
            &program,
            &Properties::new(),
            "",
            &Properties::new(),
            0,
            &mut 16_384,
        )?;
        if expanded.len() != 1 {
            return Err("A scene requires one layout root".into());
        }
        let mut root = expanded.remove(0);
        if !is_layout(&root.kind) {
            return Err("A scene requires a layout root".into());
        }
        let mut measuring = root.clone();
        if measuring.kind == "Frame"
            && !measuring.props.contains_key("columns")
            && !measuring.props.contains_key("rows")
        {
            measuring.kind = "Column".into();
        }
        let measuring = self.scene_visual(&measuring)?;
        let root_width = dimension(&measuring, 0, 360.)?;
        let root_height = dimension(&measuring, 1, 220.)?;
        root.props.insert("width".into(), number(root_width));
        root.props.insert("height".into(), number(root_height));
        let scroll = root.children.len() == 1 && root.children[0].kind == "Scroll";
        let layout = if scroll { &root.children[0] } else { &root };
        let advanced = layout.props.contains_key("columns")
            || layout.props.contains_key("rows")
            || !["Frame", "Scroll"].contains(&layout.kind.as_str())
            || layout.children.iter().any(|n| is_layout(&n.kind));
        let mut instances: Vec<(Node, Option<Node>)> = Vec::new();
        if advanced {
            let width = num(&root.props, "width", 360.)?;
            let height = num(&root.props, "height", 220.)?;
            let precomputed = if scroll {
                measuring.children.first()
            } else {
                Some(&measuring)
            };
            self.position_scene(layout, precomputed, [0., 0., width, height], &mut instances, true)?;
            for key in ["columns", "rows", "padding", "gap"] {
                root.props.remove(key);
            }
            root.props.insert("padding".into(), number(0.));
            root.props.insert("gap".into(), number(0.));
            if scroll {
                for key in ["columns", "rows", "padding", "gap"] {
                    root.children[0].props.remove(key);
                }
            }
        } else {
            let precomputed = if scroll {
                measuring.children.first()
            } else {
                Some(&measuring)
            };
            if let Some(vis) = precomputed.filter(|v| v.children.len() == layout.children.len()) {
                instances = layout
                    .children
                    .iter()
                    .cloned()
                    .zip(vis.children.iter().cloned().map(Some))
                    .collect();
            } else {
                instances = layout.children.iter().cloned().map(|n| (n, None)).collect();
            }
        }
        root.kind = "Frame".into();
        let empty = Properties::new();
        let mut mapping = Vec::new();
        let mut source_controls = Vec::new();
        let mut templates = String::from("FORMA-TEMPLATES-1\n");
        for (instance, precomputed_visual) in &instances {
            let state = match instance.props.get("__state") {
                Some(Value::Object(state)) => state,
                _ => &empty,
            };
            let event_args = match instance.props.get("__event_args") {
                Some(Value::Object(events)) => events
                    .iter()
                    .map(|(name, value)| {
                        let Value::List(values) = value else {
                            return Err("Event arguments must be a list".to_string());
                        };
                        Ok((name.clone(), values.clone()))
                    })
                    .collect::<Result<_, String>>()?,
                _ => BTreeMap::new(),
            };
            mapping.push(SceneControl {
                node: num(&instance.props, "__node", 0.)? as usize,
                key: text(&instance.props, "key", "")?,
                state: state.clone(),
                event_args,
            });
            let (visual, resolved) = match precomputed_visual {
                Some(vis) => {
                    let mut resolved = vis.props.clone();
                    let rect_props = match resolved.remove("__rect_props") {
                        Some(Value::Object(p)) => p,
                        _ => resolved.clone(),
                    };
                    for key in ["x", "y", "width", "height", "key"] {
                        if let Some(val) = instance.props.get(key) {
                            resolved.insert(key.into(), val.clone());
                        }
                    }
                    let mut visual = vis.clone();
                    visual.props = rect_props;
                    (visual, resolved)
                }
                None => self.instantiate(instance, true, state, &[])?,
            };
            let mut content = String::new();
            let mut dim_node = visual.clone();
            dim_node.props = resolved.clone();
            let bounds = [
                0.,
                0.,
                dimension(&dim_node, 0, 260.)?,
                dimension(&dim_node, 1, 70.)?,
            ];
            for child in &visual.children {
                if [
                    "Frame",
                    "Row",
                    "Column",
                    "Grid",
                    "Stack",
                    "Text",
                    "TextInput",
                    "Image",
                    "Rectangle",
                ]
                .contains(&child.kind.as_str())
                {
                    self.flatten(child, bounds, &mut content)?;
                } else {
                    content.push_str(&serialize(child)?);
                }
            }
            if instance.kind == "Slider" {
                let v = resolved
                    .get("value")
                    .ok_or("Slider value missing")?
                    .text()?
                    .trim_end_matches('%')
                    .parse::<f64>()
                    .map_err(|_| "Invalid slider value")?
                    / 100.;
                content.push_str(&format!("RangeInput {{ value:{v}; "));
                for (name, value) in [("Minimum", 0.), ("Maximum", 100.)] {
                    let mut endpoint = instance.clone();
                    endpoint
                        .props
                        .insert("value".into(), Value::Expr(format!("{value}%")));
                    let (visual, _) = self.instantiate(&endpoint, true, state, &[])?;
                    content.push_str(&format!("{name} {{ "));
                    for c in &visual.children {
                        if [
                            "Frame",
                            "Row",
                            "Column",
                            "Grid",
                            "Stack",
                            "Text",
                            "TextInput",
                            "Image",
                            "Rectangle",
                        ]
                        .contains(&c.kind.as_str())
                        {
                            self.flatten(c, bounds, &mut content)?;
                        }
                    }
                    content.push_str("} ");
                }
                content.push_str("} ");
            }
            let mut rectangle = visual.clone();
            rectangle.children.clear();
            let prefix = serialize(&rectangle)?;
            let body = prefix.strip_suffix('}').unwrap();
            let template = format!("component Button {{ {body} {content} }} }}");
            templates.push_str(&format!("{}\n{template}", template.len()));
            let standard = [
                "key",
                "x",
                "y",
                "width",
                "height",
                "text",
                "fontSize",
                "color",
                "radius",
                "disabled",
                "background",
                "hoverBackground",
                "pressedBackground",
                "disabledBackground",
                "borderWidth",
                "borderColor",
                "focusBorderColor",
                "transitionDuration",
            ];
            let mut resolved = resolved;
            resolved.insert("width".into(), number(bounds[2]));
            resolved.insert("height".into(), number(bounds[3]));
            let props = resolved
                .into_iter()
                .filter(|(k, _)| standard.contains(&k.as_str()))
                .collect();
            source_controls.push(Node {
                kind: "Button".into(),
                props,
                children: vec![],
                action: instance.action.clone(),
            });
        }
        // Scene keys must survive serialization for interaction reconciliation.
        fn scene(n: &Node) -> Result<String, String> {
            let p = n
                .props
                .iter()
                .filter(|(k, _)| {
                    !k.starts_with("__")
                        && !["minWidth", "maxWidth", "minHeight", "maxHeight"].contains(&k.as_str())
                        && !(k.as_str() == "key" && ["Frame", "Scroll"].contains(&n.kind.as_str()))
                })
                .map(|(k, v)| Ok(format!("{k}:{};", property_literal(k, v)?)))
                .collect::<Result<Vec<_>, String>>()?
                .join(" ");
            let action = n
                .action
                .as_ref()
                .map_or(String::new(), |a| format!("clicked -> {a}();"));
            Ok(format!(
                "{} {{ {p} {action} {} }}",
                n.kind,
                n.children
                    .iter()
                    .map(scene)
                    .collect::<Result<Vec<_>, _>>()?
                    .join(" ")
            ))
        }
        if scroll {
            root.children[0].children = source_controls;
        } else {
            root.children = source_controls;
        }
        Ok((
            format!("component {} {{ {} }}", self.name, scene(&root)?),
            templates,
            mapping,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn expr(path: &str) -> Value {
        Value::Expr(path.into())
    }
    fn string(value: &str) -> Value {
        Value::Text(value.into())
    }
    fn binary(op: &str, left: Value, right: Value) -> Value {
        Value::Expression(Box::new(Expression::Binary(op.into(), left, right)))
    }
    fn node(kind: &str, properties: Vec<(&str, Value)>, children: Vec<Node>) -> Node {
        Node {
            kind: kind.into(),
            props: props(properties),
            children,
            action: None,
        }
    }
    fn document(root: Node) -> Document {
        Document {
            name: "Test".into(),
            root,
            definitions: BTreeMap::from([(
                "Control".into(),
                Definition {
                    defaults: props(vec![
                        ("width", number(80.)),
                        ("height", number(30.)),
                        ("text", string("")),
                        ("disabled", Value::Bool(false)),
                    ]),
                    nodes: vec![node(
                        "Rectangle",
                        vec![],
                        vec![
                            node("Text", vec![("text", expr("props.text"))], vec![]),
                            Node {
                                action: Some("events.clicked".into()),
                                ..node("PointerArea", vec![], vec![])
                            },
                        ],
                    )],
                },
            )]),
            fallback: Properties::new(),
            images: BTreeMap::new(),
            states: vec![],
        }
    }
    fn root(children: Vec<Node>) -> Node {
        node(
            "Frame",
            vec![
                ("width", number(300.)),
                ("height", number(120.)),
                ("padding", number(0.)),
            ],
            children,
        )
    }
    fn record(id: &str, title: &str) -> Value {
        Value::Object(props(vec![("id", string(id)), ("title", string(title))]))
    }
    #[test]
    fn pure_expressions_short_circuit_and_interpolate() {
        let state = props(vec![
            ("count", number(3.)),
            ("busy", Value::Bool(false)),
            ("item", record("a", "Title")),
        ]);
        let empty = Properties::new();
        assert_eq!(
            binary("&&", Value::Bool(false), expr("state.missing"))
                .evaluate(&empty, &state)
                .unwrap(),
            Value::Bool(false)
        );
        assert_eq!(
            binary("??", Value::Null, number(4.))
                .evaluate(&empty, &state)
                .unwrap(),
            number(4.)
        );
        assert_eq!(
            Value::Interpolation(vec![
                string("Найдено: "),
                binary("+", expr("state.count"), number(2.))
            ])
            .evaluate(&empty, &state)
            .unwrap(),
            string("Найдено: 5")
        );
        assert_eq!(
            expr("item.title").evaluate(&empty, &state).unwrap(),
            string("Title")
        );
        assert!(binary("/", number(1.), number(0.))
            .evaluate(&empty, &state)
            .is_err());
        assert!(binary("+", string("a"), number(1.))
            .evaluate(&empty, &state)
            .is_err());
        let optional = Value::Expression(Box::new(Expression::Member(
            Value::Null,
            string("title"),
            true,
        )));
        assert_eq!(optional.evaluate(&empty, &state).unwrap(), Value::Null);
        let computed = Value::Expression(Box::new(Expression::Member(
            expr("state.missing"),
            expr("state.absent_index"),
            true,
        )));
        assert_eq!(computed.evaluate(&empty, &state).unwrap(), Value::Null);
        assert!(binary(
            "??",
            binary("+", expr("state.missing"), number(1.)),
            number(42.)
        )
        .evaluate(&empty, &state)
        .is_err());
    }
    #[test]
    fn standalone_primitive_wrappers_keep_nested_nodes_primitive() {
        let mut doc = document(root(vec![
            node("Text", vec![("text", expr("state.caption"))], vec![]),
            node("TextInput", vec![("value", expr("state.value"))], vec![]),
        ]));
        doc.definitions.insert(
            "Text".into(),
            Definition {
                defaults: props(vec![
                    ("text", string("")),
                    ("width", number(80.)),
                    ("height", number(30.)),
                ]),
                nodes: vec![node(
                    "Rectangle",
                    vec![],
                    vec![node("Text", vec![("text", expr("props.text"))], vec![])],
                )],
            },
        );
        doc.definitions.insert(
            "TextInput".into(),
            Definition {
                defaults: props(vec![
                    ("value", string("")),
                    ("width", number(80.)),
                    ("height", number(30.)),
                ]),
                nodes: vec![node(
                    "Rectangle",
                    vec![],
                    vec![
                        node("TextInput", vec![("value", expr("props.value"))], vec![]),
                        Node {
                            action: Some("events.clicked".into()),
                            ..node("PointerArea", vec![], vec![])
                        },
                    ],
                )],
            },
        );
        doc.states = vec![props(vec![
            ("caption", string("First")),
            ("value", string("seed")),
        ])];
        let (source, templates, mapping) = doc.render_scene().unwrap();
        assert_eq!(mapping.len(), 2);
        assert!(templates.contains("ContentText"));
        assert!(templates.contains("ContentInput"));
        let runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
        assert_eq!(runtime.text_value(1), "seed");
        doc.states[0].insert("caption".into(), string("Second"));
        assert!(doc.render().unwrap().1.contains("Second"));
    }
    #[test]
    fn contracts_check_required_properties_and_enum_members() {
        let mut doc = document(root(vec![]));
        doc.fallback.insert(
            "title".into(),
            string("Engine fallback does not satisfy required"),
        );
        let definition = doc.definitions.get_mut("Control").unwrap();
        definition.defaults.insert(
            "__contracts".into(),
            Value::Object(props(vec![
                (
                    "title",
                    Value::Object(props(vec![
                        ("type", string("String")),
                        ("required", Value::Bool(true)),
                    ])),
                ),
                ("tone", Value::Object(props(vec![("type", string("Tone"))]))),
            ])),
        );
        definition.defaults.insert(
            "__enums".into(),
            Value::Object(props(vec![(
                "Tone",
                Value::List(vec![string("Accent"), string("Danger")]),
            )])),
        );
        definition
            .defaults
            .insert("tone".into(), expr("Tone.Accent"));
        assert!(doc
            .instantiate(
                &node("Control", vec![], vec![]),
                true,
                &Properties::new(),
                &[]
            )
            .unwrap_err()
            .contains("required"));
        let valid = node(
            "Control",
            vec![("title", string("Hello")), ("tone", expr("Tone.Danger"))],
            vec![],
        );
        assert_eq!(
            doc.instantiate(&valid, true, &Properties::new(), &[])
                .unwrap()
                .1["tone"],
            string("Tone.Danger")
        );
        let bad = node(
            "Control",
            vec![("title", string("Hello")), ("tone", expr("Tone.Wrong"))],
            vec![],
        );
        assert!(doc
            .instantiate(&bad, true, &Properties::new(), &[])
            .unwrap_err()
            .contains("Unknown enum variant"));
    }
    #[test]
    fn root_scope_reacts_and_reaches_nested_scene_controls() {
        let mut doc = document(root(vec![node(
            "Control",
            vec![("text", expr("props.caption"))],
            vec![],
        )]));
        doc.root.props.insert(
            "__scope".into(),
            Value::Object(props(vec![(
                "caption",
                Value::Interpolation(vec![string("Count: "), expr("state.count")]),
            )])),
        );
        doc.states = vec![props(vec![("count", number(2.))])];
        assert!(doc.render().unwrap().0.contains("Count: 2"));
        doc.states[0].insert("count".into(), number(4.));
        assert!(doc.render().unwrap().0.contains("Count: 4"));
    }
    #[test]
    fn grouped_match_keeps_explicit_properties_last() {
        let node = node(
            "Text",
            vec![
                (
                    "__matches",
                    Value::List(vec![Value::Match(
                        Box::new(expr("state.danger")),
                        vec![
                            (
                                Value::Bool(true),
                                Value::Object(props(vec![
                                    ("color", string("#ff0000")),
                                    ("text", expr("state.missing")),
                                ])),
                            ),
                            (
                                expr("_"),
                                Value::Object(props(vec![
                                    ("color", string("#00ff00")),
                                    ("text", string("OK")),
                                ])),
                            ),
                        ],
                    )]),
                ),
                ("text", string("Override")),
            ],
            vec![],
        );
        let resolved = properties(
            &node,
            &Properties::new(),
            &props(vec![("danger", Value::Bool(true))]),
        )
        .unwrap();
        assert_eq!(resolved["color"], string("#ff0000"));
        assert_eq!(resolved["text"], string("Override"));
    }
    #[test]
    fn definition_matches_select_from_one_property_snapshot() {
        let mut doc = document(root(vec![]));
        let definition = doc.definitions.get_mut("Control").unwrap();
        definition
            .defaults
            .insert("flag".into(), Value::Bool(false));
        definition.defaults.insert(
            "__matches".into(),
            Value::List(vec![
                Value::Match(
                    Box::new(Value::Bool(true)),
                    vec![(
                        expr("_"),
                        Value::Object(props(vec![("flag", Value::Bool(true))])),
                    )],
                ),
                Value::Match(
                    Box::new(expr("props.flag")),
                    vec![
                        (
                            Value::Bool(true),
                            Value::Object(props(vec![("text", string("changed"))])),
                        ),
                        (
                            expr("_"),
                            Value::Object(props(vec![("text", string("original"))])),
                        ),
                    ],
                ),
            ]),
        );
        let (_, resolved) = doc
            .instantiate(
                &node("Control", vec![], vec![]),
                true,
                &Properties::new(),
                &[],
            )
            .unwrap();
        assert_eq!(resolved["text"], string("original"));
    }
    #[test]
    fn keyed_collection_reorders_focus_and_evaluates_event_arguments() {
        let mut doc = document(root(vec![node(
            "For",
            vec![
                ("item", string("item")),
                ("collection", expr("state.items")),
                ("key", expr("item.id")),
            ],
            vec![node(
                "Control",
                vec![
                    ("text", expr("item.title")),
                    (
                        "__event_args",
                        Value::Object(props(vec![("clicked", Value::List(vec![expr("item.id")]))])),
                    ),
                ],
                vec![],
            )],
        )]));
        doc.states = vec![props(vec![(
            "items",
            Value::List(vec![record("a", "Alpha"), record("b", "Beta")]),
        )])];
        let (source, templates, first) = doc.render_scene().unwrap();
        assert_eq!(first.len(), 2);
        assert_eq!(first[0].node, 2);
        assert_eq!(first[0].event_args["clicked"], vec![string("a")]);
        let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
        runtime.focus_control(0);
        doc.states[0].insert(
            "items".into(),
            Value::List(vec![record("b", "Beta"), record("a", "Alpha")]),
        );
        let (source, templates, second) = doc.render_scene().unwrap();
        assert_eq!(first[0].key, second[1].key);
        assert_eq!(second[1].state["__index.item"], number(1.));
        let mut next = crate::Runtime::from_sources(&source, &templates).unwrap();
        next.preserve_binding_interaction(&runtime);
        assert_eq!(next.focused_index(), 1);
        doc.states[0].insert(
            "items".into(),
            Value::List(vec![record("a", "Alpha"), record("a", "Again")]),
        );
        assert!(doc
            .render_scene()
            .unwrap_err()
            .contains("Duplicate collection key"));
    }
    #[test]
    fn flex_does_not_freeze_conflicting_minimum_and_maximum_together() {
        let row = normalize_layout(&node(
            "Row",
            vec![],
            vec![
                node(
                    "Text",
                    vec![("width", expr("*")), ("minWidth", number(80.))],
                    vec![],
                ),
                node(
                    "Text",
                    vec![("width", expr("*")), ("maxWidth", number(40.))],
                    vec![],
                ),
            ],
        ))
        .unwrap();
        assert_eq!(sizes(&row, 0, 100.).unwrap(), vec![80., 20.]);
    }
    #[test]
    fn conditional_layout_removes_the_cell_and_grid_adds_implicit_rows() {
        let mut grid = node(
            "Grid",
            vec![("columns", Value::List(vec![expr("*"), expr("*")]))],
            vec![
                node("Text", vec![], vec![]),
                node("Text", vec![], vec![]),
                node("Text", vec![], vec![]),
            ],
        );
        grid = normalize_layout(&grid).unwrap();
        assert_eq!(tracks(&grid.props, "rows").len(), 2);
        assert_eq!(grid.children[2].props["row"], number(2.));
        let mut doc = document(root(vec![node(
            "Row",
            vec![],
            vec![
                node(
                    "If",
                    vec![("condition", expr("state.show"))],
                    vec![node("Control", vec![("width", number(30.))], vec![])],
                ),
                node(
                    "Control",
                    vec![("width", expr("*")), ("maxWidth", number(100.))],
                    vec![],
                ),
                node("Control", vec![("width", expr("*"))], vec![]),
            ],
        )]));
        doc.states = vec![props(vec![("show", Value::Bool(false))])];
        let (source, templates, mapping) = doc.render_scene().unwrap();
        assert_eq!(mapping.len(), 2);
        let runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
        assert_eq!(runtime.control_bounds(0)[2], 100.);
        assert_eq!(runtime.control_bounds(1)[2], 200.);
        doc.states[0].insert("show".into(), Value::Bool(true));
        let (source, templates, mapping) = doc.render_scene().unwrap();
        assert_eq!(mapping.len(), 3);
        let runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
        assert_eq!(runtime.control_bounds(2)[2], 170.);
    }
}
