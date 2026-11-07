//! Executable component definitions emitted by the form compiler. No reflection
//! or JS is used in the application: typed model accessors supply named values.
use std::collections::BTreeMap;

mod properties;
pub use properties::{Properties, PropertyIter};

// Evaluation scopes capture immutable model data, unlike the live model map.
// Their first local mutation gets a shared-root overlay instead of owning it.
fn scope_mut<'a, 'b>(scope: &'a mut std::borrow::Cow<'b, Properties>) -> &'a mut Properties {
    if matches!(scope, std::borrow::Cow::Borrowed(_)) {
        let current=scope.as_ref();
        *scope = std::borrow::Cow::Owned(current.small_scope_clone(4096)
            .unwrap_or_else(|| current.scoped_clone()));
    }
    scope.to_mut()
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", content = "value")]
pub enum Value {
    Null,
    Bool(bool),
    Number(#[serde(with = "finite_number")] f64),
    Text(String),
    Expr(String),
    List(Vec<Value>),
    Node(Box<Node>),
    Match(Box<Value>, Vec<(Value, Value)>),
    Object(Properties),
    Expression(Box<Expression>),
    Interpolation(Vec<Value>),
}
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Expression {
    Unary(String, Value),
    Binary(String, Value, Value),
    Conditional(Value, Value, Value),
    Member(Value, Value, bool),
    Call(String, Vec<Value>),
}
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
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
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneControl {
    /// The control template's index in the original scene's preorder traversal.
    pub node: usize,
    pub key: String,
    pub state: Properties,
    pub event_args: BTreeMap<String, Vec<Value>>,
}
/// Owned Rust scene transport including layout bounds absent from control-only markup.
#[derive(Clone, Debug)]
pub struct SceneArtifact {
    pub source: String,
    pub templates: String,
    pub controls: Vec<SceneControl>,
    /// Absolute right/bottom layout bounds before root trailing padding.
    pub logical_content_end: Option<[f64; 2]>,
}
mod finite_number {
    pub fn serialize<S: serde::Serializer>(value: &f64, serializer: S) -> Result<S::Ok, S::Error> {
        if !value.is_finite() { return Err(serde::ser::Error::custom("Nonfinite scene value")); }
        serializer.serialize_f64(*value)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
        let value = <f64 as serde::Deserialize>::deserialize(deserializer)?;
        if !value.is_finite() { return Err(serde::de::Error::custom("Nonfinite scene value")); }
        Ok(value)
    }
}
const SCENE_ARTIFACT_PROTOCOL: &str = "forma-scene-artifact/1";
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SceneArtifactWire {
    protocol: String,
    source: String,
    templates: String,
    controls: Vec<SceneControl>,
    #[serde(deserialize_with = "deserialize_content_end")]
    logical_content_end: Option<[f64; 2]>,
}
fn deserialize_content_end<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<[f64; 2]>, D::Error> {
    serde::Deserialize::deserialize(deserializer)
}
impl SceneArtifact {
    pub fn to_json(&self) -> Result<String, String> {
        if self.logical_content_end.is_some_and(|end| end.iter().any(|v| !v.is_finite() || *v < 0.)) {
            return Err("Invalid logical content extent".into());
        }
        #[derive(serde::Serialize)]
        struct Wire<'a> {
            protocol: &'a str, source: &'a str, templates: &'a str,
            controls: &'a [SceneControl], logical_content_end: Option<[f64; 2]>,
        }
        serde_json::to_string(&Wire { protocol: SCENE_ARTIFACT_PROTOCOL, source: &self.source,
            templates: &self.templates, controls: &self.controls, logical_content_end: self.logical_content_end })
            .map_err(|e| e.to_string())
    }
    pub fn from_json(json: &str) -> Result<Self, String> {
        let wire: SceneArtifactWire = serde_json::from_str(json).map_err(|e| e.to_string())?;
        if wire.protocol != SCENE_ARTIFACT_PROTOCOL { return Err("Unsupported scene artifact protocol".into()); }
        if wire.logical_content_end.is_some_and(|end| end.iter().any(|v| !v.is_finite() || *v < 0.)) {
            return Err("Invalid logical content extent".into());
        }
        Ok(Self { source: wire.source, templates: wire.templates, controls: wire.controls,
            logical_content_end: wire.logical_content_end })
    }
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
    fn write_literal(&self, out: &mut String) -> Result<(), String> {
        use std::fmt::Write;
        match self {
            Self::Text(s) => {
                out.push('\'');
                for ch in s.chars() {
                    match ch {
                        '\\' => out.push_str("\\\\"),
                        '\'' => out.push_str("\\'"),
                        '\n' => out.push_str("\\n"),
                        '\r' => out.push_str("\\r"),
                        '\t' => out.push_str("\\t"),
                        c => out.push(c),
                    }
                }
                out.push('\'');
            }
            Self::Expr(s) => out.push_str(s),
            Self::Number(_) => {
                let _ = write!(out, "{}", self.number()?);
            }
            Self::Bool(v) => {
                let _ = write!(out, "{v}");
            }
            Self::List(v) => {
                for (i, item) in v.iter().enumerate() {
                    if i > 0 {
                        out.push(' ');
                    }
                    item.write_literal(out)?;
                }
            }
            Self::Node(n) => serialize_into(n, out)?,
            Self::Match(..) | Self::Expression(_) | Self::Interpolation(_) => {
                return Err("Unevaluated expression".into())
            }
            Self::Object(_) | Self::Null => return Err("Expected serializable visual value".into()),
        }
        Ok(())
    }
}
fn lookup(values: &Properties, path: &str) -> Option<Value> {
    if let Some(value) = values.get(path) {
        return Some(value.clone());
    }
    let (head, tail) = path.split_once('.')?;
    lookup_value(values.get(head)?, tail)
}
fn lookup_value(value: &Value, path: &str) -> Option<Value> {
    match value {
        Value::Object(object) => lookup(object, path),
        Value::List(items) if path == "length" => Some(number(items.len() as f64)),
        Value::List(items) => {
            let (index, tail) = path.split_once('.').map_or((path, None), |(index, tail)| (index, Some(tail)));
            let value = items.get(index.parse::<usize>().ok()?)?;
            match tail { Some(tail) => lookup_value(value, tail), None => Some(value.clone()) }
        }
        Value::Text(value) if path == "length" => Some(number(value.encode_utf16().count() as f64)),
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
                let matched = themoretheless_tokenizer_forma::evaluate::matches_constraint_pattern(&s, &pat)
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
fn is_color_property(key: &str) -> bool {
    key.as_bytes()
        .windows(5)
        .any(|w| w.eq_ignore_ascii_case(b"color"))
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
        .contains(&key)
}
fn write_property_literal(key: &str, value: &Value, out: &mut String) -> Result<(), String> {
    if let Value::Text(text) = value {
        if is_color_property(key)
            && text.starts_with('#')
            && [4, 7, 9].contains(&text.len())
            && text[1..].bytes().all(|b| b.is_ascii_hexdigit())
        {
            out.push_str(text);
            return Ok(());
        }
        if key == "overflow" && ["visible", "hidden"].contains(&text.as_str()) {
            out.push_str(text);
            return Ok(());
        }
    }
    value.write_literal(out)
}
fn serialize_into(node: &Node, out: &mut String) -> Result<(), String> {
    out.push_str(&node.kind);
    out.push_str(" { ");
    let mut first = true;
    for (k, v) in &node.props {
        if k.as_str() == "key"
            || k.starts_with("__")
            || ["minWidth", "maxWidth", "minHeight", "maxHeight"].contains(&k.as_str())
        {
            continue;
        }
        if !first {
            out.push(' ');
        }
        first = false;
        out.push_str(k);
        out.push_str(": ");
        write_property_literal(k, v, out)?;
        out.push(';');
    }
    out.push(' ');
    if let Some(a) = &node.action {
        out.push_str("clicked -> ");
        out.push_str(a);
        out.push_str("();");
    }
    out.push(' ');
    for (i, child) in node.children.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        serialize_into(child, out)?;
    }
    out.push_str(" }");
    Ok(())
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
    match value {
        None => Ok([0.; 4]),
        Some(Value::List(v)) => match v.as_slice() {
            [a] => Ok([a.number()?; 4]),
            [a, b] => {
                let (a, b) = (a.number()?, b.number()?);
                Ok([a, b, a, b])
            }
            [a, b, c] => {
                let (a, b, c) = (a.number()?, b.number()?, c.number()?);
                Ok([a, b, c, b])
            }
            [a, b, c, d] => Ok([a.number()?, b.number()?, c.number()?, d.number()?]),
            _ => Err("Expected 1..4 insets".into()),
        },
        Some(v) => Ok([v.number()?; 4]),
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
    normalize_layout_owned(n.clone())
}
fn normalize_layout_owned(mut n: Node) -> Result<Node, String> {
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
    let owned_normalized;
    let n = if matches!(n.kind.as_str(), "Row" | "Column" | "Grid" | "Stack") {
        owned_normalized = normalize_layout(n)?;
        &owned_normalized
    } else {
        n
    };
    let measured = match n.kind.as_str() {
        "Text" => {
            let fs = num(&n.props, "fontSize", num(&n.props, "font.size", 16.)?)?;
            let text = text(&n.props, "text", "")?;
            let wrap = boolean(&n.props, "wrap", false)?;
            let max_w = n.props.get("width").and_then(|v| v.number().ok()).unwrap_or(0.);
            if wrap && max_w > 0. {
                let lines = crate::text::wrap_text_ranges(&text, fs as f32, max_w as f32);
                if axis == 1 {
                    lines.len().max(1) as f64 * fs * 1.5
                } else {
                    lines.iter().map(|(_, w)| *w as f64).fold(0., f64::max)
                }
            } else if axis == 1 && text.contains('\n') {
                text.split('\n').count() as f64 * fs * 1.5
            } else {
                text.split('\n')
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
    constraint(n, axis, measured)
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
        let matched = themoretheless_tokenizer_forma::evaluate::matches_constraint_pattern(&s, &pat)
            .map_err(|e| e.to_string())?;
        if !matched {
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
// Retain evaluated Let values separately from mutable model fields. A Let
// declaration also hides an older For local with the same name.
fn insert_scene_lexical(state: &mut Properties, name: String, value: Value, expression: &Value, scope: &Properties) {
    let parents = state.get("__binding_active").cloned().unwrap_or_else(|| Value::Object(Properties::new()));
    let steps = state.get_or_insert_with("__binding_let_steps".into(), || Value::List(Vec::new()));
    if let Value::List(steps) = steps {
        steps.push(Value::Object(Properties::from([
            ("name".into(), Value::Text(name.clone())),
            ("expression".into(), expression.clone()),
            ("scope".into(), Value::Object(scope.clone())),
            ("parents".into(), parents),
        ])));
    }
    let lexicals = state.get_or_insert_with("__binding_lexicals".into(), || Value::Object(Properties::new()));
    if let Value::Object(lexicals) = lexicals { lexicals.insert(name.clone(), value.clone()); }
    if let Some(Value::Object(active)) = state.get_mut("__binding_active") { active.remove(&name); }
    for prefix in ["__collection", "__binding_key", "__binding_key_expr", "__binding_key_scope"] {
        state.unset(&format!("{prefix}.{name}"));
    }
    state.set(name, value);
}

// A displayed control no longer expands children. Keep replay data once in
// its retained frames when the root copy is exactly represented there.
fn compact_scene_control_state(mut state: Properties) -> Properties {
    let represented = match (state.get("__binding_lexicals"), state.get("__binding_let_steps"), state.get("__binding_frames")) {
        (Some(lexicals @ Value::Object(_)), Some(steps @ Value::List(_)), Some(Value::List(frames))) => frames.iter().any(|frame| {
            matches!(frame, Value::Object(frame) if frame.get("__lexicals") == Some(lexicals) && frame.get("__let_steps") == Some(steps))
        }),
        _ => false,
    };
    if represented {
        state.unset("__binding_lexicals");
        state.unset("__binding_let_steps");
    }
    state
}

fn key_part(value: &Value) -> Result<String, String> {
    let (tag, text) = match value {
        Value::Text(v) => ("s", std::borrow::Cow::Borrowed(v.as_str())),
        Value::Number(v) if v.is_finite() => ("n", std::borrow::Cow::Owned(v.to_string())),
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
        // Ordinary visual expansion only reads inherited state; lexical writes own a copy.
        let mut current_state = std::borrow::Cow::Borrowed(state);
        for node in nodes {
            match node.kind.as_str() {
                "If" => {
                    let condition = node
                        .props
                        .get("condition")
                        .ok_or("If requires a condition")?
                        .evaluate(props, &current_state)?
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
                        &current_state,
                        parents,
                        depth + 1,
                    )?);
                }
                "For" => {
                    let Value::List(items) = node
                        .props
                        .get("collection")
                        .ok_or("For requires a collection")?
                        .evaluate(props, &current_state)?
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
                                &current_state,
                                parents,
                                depth + 1,
                            )?);
                        }
                    }
                    for (index, value) in items.into_iter().enumerate() {
                        let mut state = current_state.as_ref().scoped_clone();
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
                "Let" => {
                    let name = text(&node.props, "name", "")?;
                    if !name.is_empty() {
                        if let Some(val_expr) = node.props.get("value") {
                            let val = val_expr.evaluate(props, &current_state)?;
                            scope_mut(&mut current_state).insert(name, val);
                            if !node.children.is_empty() {
                                result.extend(self.expand_sequence(
                                    &node.children,
                                    props,
                                    &current_state,
                                    parents,
                                    depth + 1,
                                )?);
                            }
                        }
                    }
                }
                "Match" => {
                    let subject = node
                        .props
                        .get("subject")
                        .ok_or("Match requires a subject")?
                        .evaluate(props, &current_state)?;
                    for branch in &node.children {
                        if branch.kind != "Branch" {
                            continue;
                        }
                        let pat = branch
                            .props
                            .get("pattern")
                            .ok_or("Match branch requires a pattern")?;
                        if pattern_matches(pat, &subject, props, &current_state, &mut Vec::new())? {
                            let guard_ok = match branch.props.get("guard") {
                                Some(g) => g.evaluate(props, &current_state)?.boolean()?,
                                None => true,
                            };
                            if guard_ok {
                                result.extend(self.expand_sequence(
                                    &branch.children,
                                    props,
                                    &current_state,
                                    parents,
                                    depth + 1,
                                )?);
                                break;
                            }
                        }
                    }
                }
                _ => result.push(self.expand(node, props, &current_state, parents, depth + 1)?),
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
            let slot_name = n
                .props
                .get("key")
                .or_else(|| n.props.get("slot"))
                .and_then(|v| match v {
                    Value::Text(s) | Value::Expr(s) => Some(s.as_str()),
                    _ => None,
                })
                .unwrap_or("content");
            let value = n
                .props
                .get("content")
                .or_else(|| props.get(slot_name))
                .cloned()
                .unwrap_or_else(|| {
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
        // Expanded properties/children replace the source tree; do not clone
        // the original subtree merely to overwrite both owned fields.
        let node = Node {
            kind: n.kind.clone(),
            props: properties(n, props, state)?
                .into_iter()
                .map(|(k, v)| Ok((k, self.expand_value(&v, props, state, parents, depth)?)))
                .collect::<Result<_, String>>()?,
            children: self.expand_sequence(&n.children, props, state, parents, depth + 1)?,
            action: n.action.clone(),
        };
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
        self.instantiate_scope(instance, top, Some(state), parents, &mut None)
    }
    fn instantiate_scene(&self, instance: &mut Node) -> Result<(Node, Properties), String> {
        let mut snapshot = instance.props.remove("__state");
        let empty_state = Properties::new();
        let supplied_state = if snapshot.is_none() { Some(&empty_state) } else { None };
        let result = self.instantiate_scope(instance, true, supplied_state, &[], &mut snapshot);
        if let Some(snapshot) = snapshot { instance.props.insert("__state".into(), snapshot); }
        result
    }
    fn instantiate_scope(
        &self, instance: &Node, top: bool, supplied_state: Option<&Properties>,
        parents: &[String], owned_snapshot: &mut Option<Value>,
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
            .chain(owned_snapshot.iter().map(|_| "__state".to_owned()))
            .collect::<std::collections::BTreeSet<_>>();
        let mut props = if top {
            self.fallback.clone()
        } else {
            Properties::new()
        };
        props.extend(definition.defaults.clone());
        props.extend(instance.props.clone());
        let transferred_snapshot = owned_snapshot.is_some();
        if let Some(snapshot) = owned_snapshot.take() { props.insert("__state".into(), snapshot); }
        let empty_state = Properties::new();
        let result = (|| {
        let state = supplied_state.unwrap_or_else(|| match props.get("__state") {
            Some(Value::Object(state)) => state, _ => &empty_state,
        });
        let mut selected = select_groups(definition.defaults.get("__matches"), &props, state)?;
        // Explicit scene metadata wins over group defaults without cloning its payload.
        if transferred_snapshot { selected.remove("__state"); }
        // Explicit instance values win over selected groups. Only keys actually
        // overwritten by a group need to be restored; transport snapshots such
        // as __state must not be cloned a second time on every instantiation.
        let overrides: Properties = selected.keys().filter_map(|key| {
            instance.props.get(key).map(|value| (key.clone(), value.clone()))
        }).collect();
        props.extend(selected);
        props.extend(overrides);
        if let Some(value) = instance.props.get("font.size") {
            props.insert("fontSize".into(), value.clone());
        }
        let mut parents = parents.to_vec();
        parents.push(instance.kind.clone());
        let state = supplied_state.unwrap_or_else(|| match props.get("__state") {
            Some(Value::Object(state)) => state, _ => &empty_state,
        });
        let resolved: Properties = props
            .iter()
            .filter(|(k, _)| !k.starts_with("__"))
            .map(|(k, v)| Ok((k.clone(), self.expand_value(v, &props, state, &parents, 0)?)))
            .collect::<Result<_, String>>()?;
        let mut contract_values = std::borrow::Cow::Borrowed(&resolved);
        if let Some(Value::Object(contracts)) = definition.defaults.get("__contracts") {
            for (key, contract) in contracts {
                if matches!(contract,Value::Object(contract) if contract.get("required")==Some(&Value::Bool(true)))
                    && !supplied.contains(key)
                {
                    if contract_values.contains_key(key) { contract_values.to_mut().remove(key); }
                }
            }
        }
        check_contracts(
            &instance.kind,
            definition.defaults.get("__contracts"),
            definition.defaults.get("__enums"),
            &contract_values,
        )?;
        let mut nodes = std::borrow::Cow::Borrowed(definition.nodes.as_slice());
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
            if !supply(nodes.to_mut(), &instance.children) {
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
        })();
        if transferred_snapshot { *owned_snapshot = props.remove("__state"); }
        result
    }
    fn flatten(&self, n: &Node, bounds: [f64; 4], out: &mut String) -> Result<(), String> {
        let [mut x, mut y, mut w, mut h] = bounds;
        let owned_normalized;
        let n = if matches!(n.kind.as_str(), "Row" | "Column" | "Grid" | "Stack") {
            owned_normalized = normalize_layout(n)?;
            &owned_normalized
        } else {
            n
        };
        let width = dimension(n, 0, w)?;
        let height = dimension(n, 1, h)?;
        x += (w - width) / 2.;
        y += (h - height) / 2.;
        w = width;
        h = height;
        let clip = |out: &mut String, radius: f64| {
            let _ = std::fmt::Write::write_fmt(
                out,
                format_args!("ContentClip {{ x:{x}; y:{y}; width:{w}; height:{h}; radius:{radius}; }} "),
            );
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
                    let _ = std::fmt::Write::write_fmt(
                        out,
                        format_args!(
                            "ContentShape {{ points:'{x} {y} {} {y} {} {} {x} {}'; color:",
                            x + w,
                            x + w,
                            y + h,
                            y + h,
                        ),
                    );
                    color.write_literal(out)?;
                    out.push_str("; } ");
                }
                for c in &n.children {
                    self.flatten(c, [x, y, w, h], out)?;
                }
                out.push_str("ContentClipEnd {} ");
            }
            "TextInput" => {
                let _ = std::fmt::Write::write_fmt(
                    out,
                    format_args!("ContentInput {{ x:{x}; y:{y}; width:{w}; height:{h}; value:"),
                );
                Value::Text(text(&n.props, "value", "")?).write_literal(out)?;
                out.push_str("; label:");
                Value::Text(text(&n.props, "label", "")?).write_literal(out)?;
                out.push_str("; placeholder:");
                Value::Text(text(&n.props, "placeholder", "")?).write_literal(out)?;
                let _ = std::fmt::Write::write_fmt(
                    out,
                    format_args!(
                        "; color:{}; placeholderColor:{}; fontSize:{}; multiline:{}; readOnly:{}; wrap:{}; }} ",
                        text(&n.props, "color", "#ffffff")?,
                        text(&n.props, "placeholderColor", "#999999")?,
                        num(&n.props, "fontSize", 15.)?,
                        boolean(&n.props, "multiline", false)?,
                        boolean(&n.props, "readOnly", false)?,
                        boolean(&n.props, "wrap", false)?,
                    ),
                );
            }
            "Text" => {
                let value = text(&n.props, "text", "")?;
                let fs = num(&n.props, "fontSize", num(&n.props, "font.size", 16.)?)?;
                let color = text(&n.props, "color", "#ffffff")?;
                let wrap = boolean(&n.props, "wrap", false)?;
                if wrap && w > 0. {
                    let wrapped = crate::text::wrap_text_ranges(&value, fs as f32, w as f32);
                    let multiline = wrapped.len() > 1 || value.contains('\n');
                    if multiline {
                        clip(out, 0.);
                    }
                    for (i, (byte_range, line_w)) in wrapped.iter().enumerate() {
                        let line = &value[byte_range.clone()];
                        let (width, height) = if multiline {
                            (*line_w as f64, fs * 1.5)
                        } else {
                            (w, h)
                        };
                        let _ = std::fmt::Write::write_fmt(
                            out,
                            format_args!(
                                "ContentText {{ x:{x}; y:{}; width:{width}; height:{height}; text:",
                                y + i as f64 * fs * 1.5
                            ),
                        );
                        Value::Text(line.into()).write_literal(out)?;
                        let _ = std::fmt::Write::write_fmt(
                            out,
                            format_args!("; color:{color}; fontSize:{fs}; }} "),
                        );
                    }
                    if multiline {
                        out.push_str("ContentClipEnd {} ");
                    }
                } else {
                    let multiline = value.contains('\n');
                    if multiline {
                        clip(out, 0.);
                    }
                    for (i, line) in value.split('\n').enumerate() {
                        let (width, height) = if multiline {
                            (
                                crate::text::measure_line(line, fs as f32)[0] as f64,
                                fs * 1.5,
                            )
                        } else {
                            (w, h)
                        };
                        let _ = std::fmt::Write::write_fmt(
                            out,
                            format_args!(
                                "ContentText {{ x:{x}; y:{}; width:{width}; height:{height}; text:",
                                y + i as f64 * fs * 1.5
                            ),
                        );
                        Value::Text(line.into()).write_literal(out)?;
                        let _ = std::fmt::Write::write_fmt(
                            out,
                            format_args!("; color:{color}; fontSize:{fs}; }} "),
                        );
                    }
                    if multiline {
                        out.push_str("ContentClipEnd {} ");
                    }
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
        let definition = self.definitions.get(&node.kind);
        // Most nodes only read defaults; materialize the merged scope only when
        // a missing-state fallback expression actually needs it for evaluation.
        let default_value = |key: &str| definition.and_then(|d| d.defaults.get(key))
            .or_else(|| self.fallback.get(key));
        let mut scope = std::borrow::Cow::Borrowed(scope);
        for meta_key in ["__enums", "__tokens", "__i18n", "__lets", "__styles"] {
            if let Some(Value::Object(meta_obj)) = default_value(meta_key) {
                let scope = scope_mut(&mut scope);
                let mut merged = match scope.remove(meta_key) {
                    Some(Value::Object(v)) => v.small_scope_clone(4096).unwrap_or_else(||v.scoped_clone()),
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
                    if let Some(default) = default_value(key) {
                        let mut defaults = self.fallback.clone();
                        if let Some(definition) = definition {
                            defaults.extend(definition.defaults.clone());
                        }
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
                let normalized = Value::Expr(format!("{}%", value * 100.));
                result.insert("value".into(), normalized);
            }
        }
        Ok(result)
    }
    fn scene_expand_sequence<'a>(
        &self, children: impl IntoIterator<Item = &'a Node>, inherited: &Properties,
        prefix: &str, scope: &Properties, depth: usize, budget: &mut usize,
    ) -> Result<Vec<Node>, String> {
        let mut state = std::borrow::Cow::Borrowed(inherited);
        let mut out = Vec::new();
        for child in children {
            if child.kind == "Let" {
                if depth > 64 { return Err("Scene composition depth exceeded".into()); }
                if *budget == 0 { return Err("Scene exceeds 16384 nodes".into()); }
                *budget -= 1;
                let name = text(&child.props, "name", "")?;
                if !name.is_empty() {
                    if let Some(expression) = child.props.get("value") {
                        let value = expression.evaluate(scope, &state)?;
                        insert_scene_lexical(scope_mut(&mut state), name, value, expression, scope);
                    }
                }
                out.extend(self.scene_expand_sequence(&child.children, &state, prefix, scope, depth + 1, budget)?);
            } else {
                out.extend(self.scene_expand(child, &state, prefix, scope, depth, budget)?);
            }
        }
        Ok(out)
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
        let mut state = std::borrow::Cow::Borrowed(inherited);
        if let Some(local) = self.states.get(id) {
            if !local.is_empty() { scope_mut(&mut state).extend(local.clone()); }
        }
        let mut scope = std::borrow::Cow::Borrowed(inherited_props);
        if let Some(Value::Object(values)) = node.props.get("__scope") {
            scope_mut(&mut scope).extend(values.clone());
            let selected = select_groups(values.get("__matches"), &scope, &state)?;
            scope_mut(&mut scope).extend(selected);
            let resolved = scope
                .iter()
                .filter(|(key, _)| !key.starts_with("__"))
                .map(|(key, value)| Ok((key.clone(), value.evaluate(&scope, &state)?)))
                .collect::<Result<Properties, String>>()?;
            let mut contract_values = std::borrow::Cow::Borrowed(&resolved);
            if let Some(Value::Object(contracts)) = values.get("__contracts") {
                for (key, contract) in contracts {
                    if matches!(contract,Value::Object(contract) if contract.get("required")==Some(&Value::Bool(true)))
                        && !values.contains_key(key)
                    {
                        if contract_values.contains_key(key) { contract_values.to_mut().remove(key); }
                    }
                }
            }
            check_contracts(
                &self.name,
                scope.get("__contracts"),
                scope.get("__enums"),
                &contract_values,
            )?;
            scope_mut(&mut scope).extend(resolved);
        }
        match node.kind.as_str() {
            "If" => {
                let condition = node
                    .props
                    .get("condition")
                    .ok_or("If requires a condition")?
                    .evaluate(&scope, &state)?
                    .boolean()?;
                if condition {
                    self.scene_expand_sequence(node.children.iter().filter(|n| n.kind != "Else"), &state, prefix, &scope, depth + 1, budget)
                } else if let Some(branch) = node.children.iter().find(|n| n.kind == "Else") {
                    self.scene_expand_sequence(&branch.children, &state, prefix, &scope, depth + 1, budget)
                } else { Ok(Vec::new()) }
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
                let collection_expr = node
                    .props
                    .get("collection")
                    .and_then(|v| v.text().ok());
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
                // Small loops with little owned data keep the cheaper flat
                // map path. Large loops/payloads retain shared-root overlays.
                let mut local = std::borrow::Cow::Owned(
                    if items.len()<=32 { state.small_scope_clone(4096) } else { None }
                        .unwrap_or_else(|| state.as_ref().scoped_clone())
                );
                // These captured outer locals are identical for every item.
                // Materialize them only after the first valid key, then share
                // the immutable map across retained binding frames.
                let mut captured_locals: Option<Properties> = None;
                let binding_metadata = [
                    format!("__binding_key.{item}"),
                    format!("__binding_key_expr.{item}"),
                    format!("__binding_key_scope.{item}"),
                ];
                for (index, value) in items.into_iter().enumerate() {
                    let local = scope_mut(&mut local);
                    // Key evaluation sees the inherited metadata, never the
                    // previous iteration's key (also when nested For shadows item).
                    for name in &binding_metadata {
                        match state.get(name) {
                            Some(value) => { local.set(name.clone(), value.clone()); }
                            None => { local.unset(name); }
                        }
                    }
                    local.set(item.clone(), value);
                    if let Some(Value::Object(lexicals)) = local.get_mut("__binding_lexicals") {
                        if lexicals.contains_key(&item) {
                            let mut masked=lexicals.scoped_clone();
                            masked.unset(&item);
                            *lexicals=masked;
                        }
                    }
                    local.set(format!("__index.{item}"), number(index as f64));
                    if let Some(col_expr) = &collection_expr {
                        local.set(format!("__collection.{item}"), Value::Text(col_expr.clone()));
                    }
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
                    // Retain key semantics, not just the old positional index,
                    // so a recovery binding map can resolve the same live item.
                    local.set(format!("__binding_key.{item}"), Value::Text(key.clone()));
                    local.set(format!("__binding_key_expr.{item}"), node.props["key"].clone());
                    local.set(format!("__binding_key_scope.{item}"), Value::Object(scope.as_ref().clone()));
                    let mut frames = match state.get("__binding_frames") {
                        Some(Value::List(frames)) => frames.clone(), _ => Vec::new(),
                    };
                    let mut active = match state.get("__binding_active") {
                        Some(Value::Object(active)) => active.clone(), _ => Properties::new(),
                    };
                    // Lexicals are retained in __lexicals and replayed over locals.
                    // Avoid a second owned copy of each lexical collection here.
                    let locals = captured_locals.get_or_insert_with(|| state.iter().filter(|(name, _)| {
                        !name.starts_with("__")
                            && !self.states.first().is_some_and(|root| root.contains_key(*name))
                            && !matches!(state.get("__binding_lexicals"), Some(Value::Object(lexicals)) if lexicals.contains_key(*name))
                    }).map(|(name, value)| (name.clone(), value.clone())).collect()).clone();
                    let mut frame = Properties::from([
                        ("__item".into(), Value::Text(item.clone())),
                        ("__parents".into(), Value::Object(active.clone())),
                        ("__locals".into(), Value::Object(locals)),
                    ]);
                    if let Some(Value::Object(lexicals)) = state.get("__binding_lexicals") {
                        frame.insert("__lexicals".into(), Value::Object(lexicals.clone()));
                        if let Some(steps) = state.get("__binding_let_steps") {
                            frame.insert("__let_steps".into(), steps.clone());
                        }
                    }
                    for name in [format!("__collection.{item}"), format!("__binding_key.{item}"), format!("__binding_key_expr.{item}"), format!("__binding_key_scope.{item}")] {
                        if let Some(value) = local.get(&name) { frame.insert(name, value.clone()); }
                    }
                    active.insert(item.clone(), number(frames.len() as f64));
                    frames.push(Value::Object(frame));
                    local.set("__binding_frames".into(), Value::List(frames));
                    local.set("__binding_active".into(), Value::Object(active));
                    let prefix = format!("{prefix}@{id}:{key}/");
                    out.extend(self.scene_expand_sequence(node.children.iter().filter(|n| n.kind != "Empty"), local, &prefix, &scope, depth + 1, budget)?);
                }
                Ok(out)
            }
            "Else" | "Empty" => Ok(Vec::new()),
            "Let" => {
                let name = text(&node.props, "name", "")?;
                let mut out = Vec::new();
                if !name.is_empty() {
                    if let Some(val_expr) = node.props.get("value") {
                        let val = val_expr.evaluate(&scope, &state)?;
                        let mut next_state = state.as_ref().scoped_clone();
                        insert_scene_lexical(&mut next_state, name, val, val_expr, &scope);
                        out.extend(self.scene_expand_sequence(&node.children, &next_state, prefix, &scope, depth + 1, budget)?);
                    }
                }
                Ok(out)
            }
            "Match" => {
                let subject = node
                    .props
                    .get("subject")
                    .ok_or("Match requires a subject")?
                    .evaluate(&scope, &state)?;
                let mut out = Vec::new();
                for branch in &node.children {
                    if branch.kind != "Branch" {
                        continue;
                    }
                    let pat = branch
                        .props
                        .get("pattern")
                        .ok_or("Match branch requires a pattern")?;
                    if pattern_matches(pat, &subject, &scope, &state, &mut Vec::new())? {
                        let guard_ok = match branch.props.get("guard") {
                            Some(g) => g.evaluate(&scope, &state)?.boolean()?,
                            None => true,
                        };
                        if guard_ok {
                            out.extend(self.scene_expand_sequence(&branch.children, &state, prefix, &scope, depth + 1, budget)?);
                            break;
                        }
                    }
                }
                Ok(out)
            }
            _ => {
                let mut output = Node {
                    kind: node.kind.clone(),
                    props: self.scene_properties(node, &scope, &state)?,
                    children: Vec::new(),
                    action: node.action.clone(),
                };
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
                    output.children = self.scene_expand_sequence(&node.children, &state, prefix, &scope, depth + 1, budget)?;
                }
                // Children and resolved properties no longer borrow this scope.
                // Move a local owned state into its public control snapshot;
                // inherited shared state is cloned only when ownership is needed.
                // Layout-only nodes do not consume snapshots.
                if !is_layout(&node.kind) {
                    output.props.insert("__state".into(), Value::Object(state.into_owned()));
                }
                Ok(vec![output])
            }
        }
    }
    fn scene_visual(&self, node: &mut Node) -> Result<Node, String> {
        if is_layout(&node.kind) {
            let visual = Node {
                kind: node.kind.clone(),
                props: node.props.clone(),
                children: node.children.iter_mut().map(|n| self.scene_visual(n)).collect::<Result<_, _>>()?,
                action: node.action.clone(),
            };
            normalize_layout_owned(visual)
        } else {
            self.instantiate_scene(node)
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
        mut node: Node,
        precomputed_visual: Option<Node>,
        bounds: [f64; 4],
        instances: &mut Vec<(Node, Option<Node>)>,
        root_legacy: bool,
        logical_end: &mut [f64; 2],
        scroll_content: bool,
    ) -> Result<(), String> {
        // Oversized scroll content must overflow toward positive offsets; centering
        // it at a negative origin makes the leading half unreachable by scrolling.
        let align = |available: f64, size: f64| {
            let offset = (available - size) / 2.;
            if scroll_content { offset.max(0.) } else { offset }
        };
        if is_layout(&node.kind) {
            logical_end[0] = logical_end[0].max(bounds[0] + bounds[2]);
            logical_end[1] = logical_end[1].max(bounds[1] + bounds[3]);
        }
        if !is_layout(&node.kind) {
            let visual = match precomputed_visual {
                Some(v) => v,
                None => self.scene_visual(&mut node)?
            };
            let width = dimension(&visual, 0, bounds[2])?;
            let height = dimension(&visual, 1, bounds[3])?;
            for (key, value) in [
                ("x", bounds[0] + align(bounds[2], width)),
                ("y", bounds[1] + align(bounds[3], height)),
                ("width", width),
                ("height", height),
            ] {
                node.props.insert(key.into(), number(value));
            }
            for key in ["cell", "row", "column"] {
                node.props.remove(key);
            }
            instances.push((node, Some(visual)));
            return Ok(());
        }
        if !root_legacy && boolean(&node.props, "clip", false)? {
            return Err("Nested scene clipping is unsupported; use the root Frame clip".into());
        }
        if root_legacy
            && ["Frame", "Scroll"].contains(&node.kind.as_str())
            && !node.props.contains_key("columns")
            && !node.props.contains_key("rows")
        {
            node.kind = "Column".into();
        }
        let mut node = normalize_layout_owned(node)?;
        let visual = match precomputed_visual {
            Some(v) if v.kind == node.kind && v.children.len() == node.children.len() => v,
            _ => self.scene_visual(&mut node)?
        };
        let [mut x, mut y, mut width, mut height] = bounds;
        // A container consumes its allocated cell; its explicit dimensions are resolved by its parent.
        let pad = insets(node.props.get("padding"))?;
        x += pad[3];
        y += pad[0];
        width = (width - pad[1] - pad[3]).max(0.);
        height = (height - pad[0] - pad[2]).max(0.);
        let cols = sizes(&visual, 0, width)?;
        let rows = sizes(&visual, 1, height)?;
        let gap = gaps(&node.props)?;
        for (child, child_visual) in node.children.into_iter().zip(visual.children) {
            let ci = cell(&child, 0, cols.len())?;
            let ri = cell(&child, 1, rows.len())?;
            let cw = ci.map_or(width, |i| cols[i]);
            let ch = ri.map_or(height, |i| rows[i]);
            let mut cx = x + ci.map_or(0., |i| cols[..i].iter().sum::<f64>() + i as f64 * gap[1]);
            let mut cy = y + ri.map_or(0., |i| rows[..i].iter().sum::<f64>() + i as f64 * gap[0]);
            if is_layout(&child.kind) {
                let w = dimension(&child_visual, 0, cw)?;
                let h = dimension(&child_visual, 1, ch)?;
                cx += align(cw, w);
                cy += align(ch, h);
                self.position_scene(child, Some(child_visual), [cx, cy, w, h], instances, false, logical_end, scroll_content)?;
            } else {
                self.position_scene(child, Some(child_visual), [cx, cy, cw, ch], instances, false, logical_end, scroll_content)?;
            }
        }
        Ok(())
    }
    /// Re-evaluate the retained program. The binding owner keeps the last valid scene on error.
    pub fn render(&self) -> Result<(String, String), String> {
        self.render_scene()
            .map(|(source, templates, _)| (source, templates))
    }
    /// Current control identity without visual instantiation/layout/serialization.
    /// A failed visual must not authorize bindings from a branch no longer selected.
    pub(crate) fn binding_scene_controls(&self) -> Result<Vec<SceneControl>, String> {
        let program = annotated_scene_source(&self.root, &mut 0, 0)?;
        let mut expanded = self.scene_expand(&program, &Properties::new(), "", &Properties::new(), 0, &mut 16_384)?;
        if expanded.len() != 1 || !is_layout(&expanded[0].kind) {
            return Err("A scene requires one layout root".into());
        }
        fn collect(mut node: Node, controls: &mut Vec<SceneControl>) -> Result<(), String> {
            if is_layout(&node.kind) {
                for child in node.children { collect(child, controls)?; }
            } else {
                let id = num(&node.props, "__node", 0.)? as usize;
                let key = text(&node.props, "key", "")?;
                let state = match node.props.remove("__state") {
                    Some(Value::Object(state)) => state, _ => Properties::new(),
                };
                let event_args = match node.props.remove("__event_args") {
                    Some(Value::Object(events)) => events.into_iter().map(|(name, value)| {
                        let Value::List(values) = value else { return Err("Event arguments must be a list".to_string()); };
                        Ok((name, values))
                    }).collect::<Result<_, String>>()?,
                    _ => BTreeMap::new(),
                };
                controls.push(SceneControl { node: id, key, state: compact_scene_control_state(state), event_args });
            }
            Ok(())
        }
        let mut controls = Vec::new();
        collect(expanded.remove(0), &mut controls)?;
        Ok(controls)
    }
    pub fn render_scene(&self) -> Result<(String, String, Vec<SceneControl>), String> {
        self.render_scene_with_extents().map(|(source, templates, controls, _)| (source, templates, controls))
    }
    /// Render an owned artifact for `Runtime::from_scene_artifact`, retaining empty layout extents.
    pub fn render_scene_artifact(&self) -> Result<SceneArtifact, String> {
        let (source, templates, controls, logical_content_end) = self.render_scene_with_extents()?;
        Ok(SceneArtifact { source, templates, controls, logical_content_end })
    }
    /// Runtime metadata retains logical layout bounds that serialized control leaves cannot express.
    pub(crate) fn render_scene_with_extents(&self) -> Result<(String, String, Vec<SceneControl>, Option<[f64; 2]>), String> {
        let program = annotated_scene_source(&self.root, &mut 0, 0)?;
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
        let measuring = if root.kind == "Frame"
            && !root.props.contains_key("columns") && !root.props.contains_key("rows")
        {
            let kind = std::mem::replace(&mut root.kind, "Column".into());
            let measured = self.scene_visual(&mut root);
            root.kind = kind;
            measured?
        } else { self.scene_visual(&mut root)? };
        let root_width = dimension(&measuring, 0, 360.)?;
        let root_height = dimension(&measuring, 1, 220.)?;
        root.props.insert("width".into(), number(root_width));
        root.props.insert("height".into(), number(root_height));
        let scroll = root.children.len() == 1 && root.children[0].kind == "Scroll";
        let scroll_viewport = if scroll {
            let layout = &root.children[0];
            for key in layout.props.keys() {
                if !key.starts_with("__") && !["key", "width", "height", "minWidth", "maxWidth", "minHeight", "maxHeight", "padding", "gap", "columns", "rows"].contains(&key.as_str()) {
                    return Err(format!("Unsupported root Scroll property: {key}"));
                }
            }
            if layout.action.is_some() { return Err("Root Scroll actions are unsupported".into()); }
            let padding = insets(root.props.get("padding"))?;
            let available = [root_width - padding[1] - padding[3], root_height - padding[0] - padding[2]];
            let visual = measuring.children.first().ok_or("Missing Scroll visual")?;
            let width = dimension(visual, 0, available[0])?;
            let height = dimension(visual, 1, available[1])?;
            if width < 0. || height < 0. || width > available[0] || height > available[1] {
                return Err("Root Scroll viewport must fit inside the scene content area".into());
            }
            Some([padding[3] + (available[0] - width) / 2., padding[0] + (available[1] - height) / 2., width, height])
        } else { None };
        let layout = if scroll { &root.children[0] } else { &root };
        let advanced = scroll || layout.props.contains_key("columns")
            || layout.props.contains_key("rows")
            || !["Frame", "Scroll"].contains(&layout.kind.as_str())
            || layout.children.iter().any(|n| is_layout(&n.kind));
        let mut instances: Vec<(Node, Option<Node>)> = Vec::new();
        let mut logical_end = [0_f64; 2];
        if advanced {
            let width = num(&root.props, "width", 360.)?;
            let height = num(&root.props, "height", 220.)?;
            let precomputed = if scroll {
                measuring.children.into_iter().next()
            } else {
                Some(measuring)
            };
            let mut positioned = Node {
                kind: layout.kind.clone(),
                props: layout.props.clone(),
                children: Vec::new(),
                action: layout.action.clone(),
            };
            // Retain only the root/Scroll shell for final scene serialization.
            // Expanded children and their owned snapshots move into positioning.
            positioned.children = if scroll {
                std::mem::take(&mut root.children[0].children)
            } else {
                std::mem::take(&mut root.children)
            };
            self.position_scene(positioned, precomputed, scroll_viewport.unwrap_or([0., 0., width, height]), &mut instances, true, &mut logical_end, scroll)?;
            for key in ["columns", "rows", "padding", "gap"] {
                root.props.remove(key);
            }
            root.props.insert("padding".into(), number(0.));
            root.props.insert("gap".into(), number(0.));
            if scroll {
                let [x, y, width, height] = scroll_viewport.unwrap();
                root.props.insert("padding".into(), Value::List(vec![number(y), number(root_width - x - width), number(root_height - y - height), number(x)]));
                for key in ["columns", "rows", "padding", "gap"] {
                    root.children[0].props.remove(key);
                }
            }
        } else {
            let precomputed = if scroll {
                measuring.children.into_iter().next()
            } else {
                Some(measuring)
            };
            // The root shell is serialized separately; transfer its expanded
            // children and their snapshots just as the positioned path does.
            let children = if scroll {
                std::mem::take(&mut root.children[0].children)
            } else {
                std::mem::take(&mut root.children)
            };
            if let Some(vis) = precomputed.filter(|v| v.children.len() == children.len()) {
                instances = children.into_iter().zip(vis.children.into_iter().map(Some)).collect();
            } else {
                instances = children.into_iter().map(|n| (n, None)).collect();
            }
        }
        root.kind = "Frame".into();
        let empty = Properties::new();
        let mut mapping = Vec::with_capacity(instances.len());
        let mut source_controls = Vec::with_capacity(instances.len());
        let mut templates = String::with_capacity(32 + instances.len() * 256);
        templates.push_str("FORMA-TEMPLATES-1\n");
        for (mut instance, precomputed_visual) in instances {
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
            let node = num(&instance.props, "__node", 0.)? as usize;
            let key = text(&instance.props, "key", "")?;
            let (mut visual, mut resolved) = match precomputed_visual {
                Some(mut visual) => {
                    let mut resolved = std::mem::take(&mut visual.props);
                    let rect_props = match resolved.remove("__rect_props") {
                        Some(Value::Object(p)) => p,
                        _ => resolved.clone(),
                    };
                    for key in ["x", "y", "width", "height", "key"] {
                        if let Some(val) = instance.props.get(key) {
                            resolved.insert(key.into(), val.clone());
                        }
                    }
                    visual.props = rect_props;
                    (visual, resolved)
                }
                None => self.instantiate_scene(&mut instance)?,
            };
            let state = match instance.props.get("__state") {
                Some(Value::Object(state)) => state,
                _ => &empty,
            };
            let mut content = String::with_capacity(128);
            // Measure the same children with resolved instance properties,
            // then restore rectangle properties before flattening/serialization.
            std::mem::swap(&mut visual.props, &mut resolved);
            let bounds = [
                0.,
                0.,
                dimension(&visual, 0, 260.)?,
                dimension(&visual, 1, 70.)?,
            ];
            std::mem::swap(&mut visual.props, &mut resolved);
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
                    serialize_into(child, &mut content)?;
                    content.push(' ');
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
                let _ = std::fmt::Write::write_fmt(&mut content, format_args!("RangeInput {{ value:{v}; "));
                for (name, value) in [("Minimum", 0.), ("Maximum", 100.)] {
                    let mut endpoint = instance.clone();
                    endpoint
                        .props
                        .insert("value".into(), Value::Expr(format!("{value}%")));
                    let (visual, _) = self.instantiate(&endpoint, true, state, &[])?;
                    content.push_str(name);
                    content.push_str(" { ");
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
            let rectangle = Node {
                kind: visual.kind.clone(),
                props: visual.props.clone(),
                children: Vec::new(),
                action: visual.action.clone(),
            };
            let mut template = String::with_capacity(64 + content.len());
            template.push_str("component Button { ");
            serialize_into(&rectangle, &mut template)?;
            template.pop();
            template.push_str(&content);
            template.push_str(" } }");
            let _ = std::fmt::Write::write_fmt(&mut templates, format_args!("{}\n{template}", template.len()));
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
            // Templates (including Slider endpoints) may read props.__state.
            // Transfer its owned snapshot only after their last evaluation.
            let state = match instance.props.remove("__state") {
                Some(Value::Object(state)) => state,
                _ => Properties::new(),
            };
            mapping.push(SceneControl {
                node,
                key,
                state: compact_scene_control_state(state),
                event_args,
            });
        }
        // Scene keys must survive serialization for interaction reconciliation.
        fn write_scene(n: &Node, out: &mut String) -> Result<(), String> {
            out.push_str(&n.kind);
            out.push_str(" { ");
            let mut first = true;
            for (k, v) in &n.props {
                if n.kind == "Scroll" || k.starts_with("__")
                    || ["minWidth", "maxWidth", "minHeight", "maxHeight"].contains(&k.as_str())
                    || (k.as_str() == "key" && ["Frame", "Scroll"].contains(&n.kind.as_str()))
                {
                    continue;
                }
                if !first {
                    out.push(' ');
                }
                first = false;
                out.push_str(k);
                out.push(':');
                write_property_literal(k, v, out)?;
                out.push(';');
            }
            out.push(' ');
            if let Some(a) = &n.action {
                out.push_str("clicked -> ");
                out.push_str(a);
                out.push_str("();");
            }
            out.push(' ');
            for (i, child) in n.children.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                write_scene(child, out)?;
            }
            out.push_str(" }");
            Ok(())
        }
        if scroll {
            root.children[0].children = source_controls;
        } else {
            root.children = source_controls;
        }
        let control_count = if scroll {
            root.children[0].children.len()
        } else {
            root.children.len()
        };
        let mut scene_source = String::with_capacity(256 + control_count * 128);
        scene_source.push_str("component ");
        scene_source.push_str(&self.name);
        scene_source.push_str(" { ");
        write_scene(&root, &mut scene_source)?;
        scene_source.push_str(" }");
        Ok((scene_source, templates, mapping, scroll.then_some(logical_end)))
    }

    /// Parses an SVG document (`svg`, `rect`, `circle`, `ellipse`, `line`, `polyline`, `polygon`)
    /// into a vector [`Image`] suitable for `Document.images`.
    pub fn parse_svg_image(svg: &str) -> Result<Image, String> {
        parse_svg_shapes(svg)
    }

    /// Compiles a project of `.ui` and `.svg` files directly in Rust (resolving `use` imports,
    /// `base` component inheritance, `override` slots, `tokens`, `i18n`, `styles`, `enum`/`struct`
    /// contracts, and `.design.ui` state patches) into an executable [`Document`].
    pub fn from_ui_files(
        files: &BTreeMap<String, String>,
        entry_file: &str,
        initial_state: &Properties,
    ) -> Result<Self, String> {
        compile_ui_files_to_document(files, entry_file, initial_state)
    }

    /// Compiles a JSON map of `.ui`/`.svg` files (`{"App.ui": "...", ...}`) and a JSON state map (`{}`)
    /// directly into an executable [`Document`].
    pub fn from_ui_files_json(
        files_json: &str,
        entry_file: &str,
        state_json: &str,
    ) -> Result<Self, String> {
        let files: BTreeMap<String, String> =
            serde_json::from_str(files_json).map_err(|e| format!("Invalid files JSON: {e}"))?;
        let state_val: serde_json::Value = if state_json.trim().is_empty() {
            serde_json::Value::Object(serde_json::Map::new())
        } else {
            serde_json::from_str(state_json).map_err(|e| format!("Invalid state JSON: {e}"))?
        };
        let initial_state = json_to_doc_props(&state_val)?;
        Self::from_ui_files(&files, entry_file, &initial_state)
    }

    /// Synchronizes two-way `<->` bindings (`value`, `checked`, `selected`) from a live [`crate::Runtime`]
    /// back into `self.states` (including nested `state.a.b` paths).
    /// Returns the number of bound properties that changed. An error leaves
    /// all state scopes unchanged; successful writes commit as one batch.
    pub fn sync_runtime_bindings(
        &mut self,
        runtime: &crate::Runtime,
        activated_index: Option<usize>,
    ) -> Result<usize, String> {
        let (_, _, controls) = self.render_scene()?;
        self.sync_runtime_bindings_from_scene(runtime, activated_index, &controls)
    }

    /// Use the last displayed binding map when current state cannot render.
    /// The host must supply controls belonging to this document's live scene.
    pub(crate) fn sync_runtime_bindings_from_scene(
        &mut self,
        runtime: &crate::Runtime,
        activated_index: Option<usize>,
        controls: &[SceneControl],
    ) -> Result<usize, String> {
        self.sync_runtime_binding_events(runtime, activated_index.as_slice(), controls)
    }

    /// Read current widget values once, then replay buffered activations in order.
    /// All writes share a single atomic state transaction.
    pub(crate) fn sync_runtime_binding_events(
        &mut self,
        runtime: &crate::Runtime,
        activations: &[usize],
        controls: &[SceneControl],
    ) -> Result<usize, String> {
        let annotated = annotated_scene_source(&self.root, &mut 0, 0)?;
        // Scene expansion may have reordered/removed controls since the live
        // widgets captured their events. Match stable keys before using indices.
        let mut runtime_keys = BTreeMap::new();
        for index in 0..runtime.control_count() {
            let key = runtime.control_key(index);
            if runtime_keys.insert(key.clone(), Some(index)).is_some() {
                runtime_keys.insert(key, None);
            }
        }
        let mut bound_controls = vec![None; runtime.control_count()];
        let mut scene_keys = std::collections::BTreeSet::new();
        for control in controls {
            if !scene_keys.insert(&control.key) {
                return Err(format!("Ambiguous binding control key: {}", control.key));
            }
            match runtime_keys.get(&control.key) {
                Some(Some(index)) => bound_controls[*index] = Some(control),
                Some(None) => {
                    return Err(format!("Ambiguous runtime binding key: {}", control.key))
                }
                None => {} // Newly inserted controls do not own a live widget yet.
            }
        }
        // Commit one root scope only after every binding has validated. The
        // staged values also preserve sequential/alias semantics within a batch.
        let empty_state = Properties::new();
        let mut state = std::borrow::Cow::Borrowed(self.states.first().unwrap_or(&empty_state));
        let mut changed = 0usize;
        let mut key_indexes = BindingKeyIndexes::new();
        for activated_index in std::iter::once(None).chain(activations.iter().copied().map(Some)) {
            let start = activated_index.unwrap_or(0);
            let end = activated_index.map_or(bound_controls.len(), |index| {
                index.saturating_add(1).min(bound_controls.len())
            });
            for idx in start..end {
                let Some(ctrl) = bound_controls[idx] else {
                    continue;
                };
                let Some(src_node) = find_annotated_node(&annotated, ctrl.node) else {
                    continue;
                };
                let Some(Value::Object(bindings)) = src_node.props.get("__bindings") else {
                    continue;
                };
                for (prop_name, path_val) in bindings {
                    let path = path_val.text()?;
                    let applies = match prop_name.as_str() {
                        "value" => activated_index.is_none(),
                        "checked" | "selected" => activated_index == Some(idx),
                        _ => false,
                    };
                    if !applies {
                        continue;
                    }
                    let collection_target = if path.starts_with("state.") {
                        None
                    } else if let Some((item, tail)) = path.split_once('.') {
                        let Some((collection, index)) = resolve_binding_item_cached(ctrl, item, &state, &mut key_indexes)?
                        else {
                            continue;
                        };
                        Some((collection, index, tail.to_owned()))
                    } else {
                        None
                    };
                    if prop_name == "value" && !runtime.binding_widget_changed(idx) {
                        let present = if let Some(path) = path.strip_prefix("state.") {
                            lookup_binding_ref(&state, path).is_some() || lookup(&state, path).is_some()
                        } else if let Some((collection, index, tail)) = &collection_target {
                            match lookup_binding_ref(&state, collection) {
                                Some(Value::List(items)) => match items.get(*index) {
                                    Some(Value::Object(item)) => lookup_binding_ref(item, tail).is_some() || lookup(item, tail).is_some(),
                                    _ => false,
                                },
                                _ => false,
                            }
                        } else {
                            false
                        };
                        if present {
                            continue;
                        }
                    }
                    let new_val = match prop_name.as_str() {
                        "value" if activated_index.is_none() => {
                            if runtime.control_editable(idx) {
                                Some(Value::Text(runtime.text_value(idx)))
                            } else {
                                let r = runtime.range_value(idx);
                                if r.is_finite() {
                                    Some(number(r as f64))
                                } else {
                                    None
                                }
                            }
                        }
                        "checked" if activated_index == Some(idx) => {
                            let current =
                                if let Some((collection, index, tail)) = &collection_target {
                                    match lookup_binding_ref(&state, collection) {
                                        Some(Value::List(items)) => match items.get(*index) {
                                            Some(Value::Object(item)) => lookup_binding_ref(item, tail).cloned(),
                                            _ => None,
                                        },
                                        _ => None,
                                    }
                                } else {
                                    eval(
                                        &Value::Expr(path.clone()),
                                        &Properties::new(),
                                        &state,
                                        &mut Vec::new(),
                                    )
                                    .ok()
                                };
                            let checked = current
                                .and_then(|value| value.boolean().ok())
                                .unwrap_or(false);
                            Some(Value::Bool(!checked))
                        }
                        "selected" if activated_index == Some(idx) => Some(Value::Bool(true)),
                        _ => None,
                    };
                    let Some(val) = new_val else {
                        continue;
                    };
                    if let Some(state_path) = path.strip_prefix("state.") {
                        if write_state_path(state.to_mut(), state_path, val) {
                            changed += 1;
                            key_indexes.clear();
                        }
                    } else if let Some((collection, index, tail)) = collection_target {
                        if write_collection_item_path(state.to_mut(), &collection, index, &tail, val) {
                            changed += 1;
                            key_indexes.clear();
                        }
                    }
                }
            }
        }
        match state {
            std::borrow::Cow::Owned(state) => {
                if let Some(root) = self.states.first_mut() {
                    *root = state;
                } else {
                    self.states.push(state);
                }
            }
            std::borrow::Cow::Borrowed(_) => {
                if self.states.is_empty() { self.states.push(Properties::new()); }
            }
        }
        runtime.acknowledge_binding_widgets();
        Ok(changed)
    }

    /// Exports the root state map (`self.states[0]`) as a JSON object string.
    pub fn state_json(&self) -> Result<String, String> {
        let empty = Properties::new();
        let root_state = self.states.first().unwrap_or(&empty);
        let json_val = doc_props_to_json(root_state)?;
        serde_json::to_string(&json_val).map_err(|e| e.to_string())
    }
}

// Borrow real model fields instead of cloning a whole collection to inspect a
// member. Synthetic length properties are intentionally not collection targets.
fn lookup_binding_ref<'a>(values: &'a Properties, path: &str) -> Option<&'a Value> {
    if let Some(value) = values.get(path) { return Some(value); }
    let (head, tail) = path.split_once('.')?;
    lookup_binding_value_ref(values.get(head)?, tail)
}
fn lookup_binding_value_ref<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    match value {
        Value::Object(object) => lookup_binding_ref(object, path),
        Value::List(items) => {
            let (index, tail) = path.split_once('.').map_or((path, None), |(index, tail)| (index, Some(tail)));
            let value = items.get(index.parse::<usize>().ok()?)?;
            match tail { Some(tail) => lookup_binding_value_ref(value, tail), None => Some(value) }
        }
        _ => None,
    }
}
fn lookup_binding_mut<'a>(values: &'a mut Properties, path: &str) -> Option<&'a mut Value> {
    if values.contains_key(path) { return values.get_mut(path); }
    let (head, tail) = path.split_once('.')?;
    lookup_binding_value_mut(values.get_mut(head)?, tail)
}
fn lookup_binding_value_mut<'a>(value: &'a mut Value, path: &str) -> Option<&'a mut Value> {
    match value {
        Value::Object(object) => lookup_binding_mut(object, path),
        Value::List(items) => {
            let (index, tail) = path.split_once('.').map_or((path, None), |(index, tail)| (index, Some(tail)));
            let value = items.get_mut(index.parse::<usize>().ok()?)?;
            match tail { Some(tail) => lookup_binding_value_mut(value, tail), None => Some(value) }
        }
        _ => None,
    }
}

/// Resolve the retained item's stable key against the current root collection.
/// Never route a stale positional index into a different object.
type BindingKeyIndexes = BTreeMap<String, BTreeMap<String, BTreeMap<String, usize>>>;

#[cfg(test)]
fn resolve_binding_item(
    control: &SceneControl,
    item: &str,
    state: &Properties,
) -> Result<Option<(String, usize)>, String> {
    resolve_binding_item_cached(control, item, state, &mut BindingKeyIndexes::new())
}

fn resolve_binding_item_cached(
    control: &SceneControl,
    item: &str,
    state: &Properties,
    indexes: &mut BindingKeyIndexes,
) -> Result<Option<(String, usize)>, String> {
    match (control.state.get("__binding_frames"), control.state.get("__binding_active")) {
        (Some(Value::List(frames)), Some(Value::Object(active))) => {
            if frames.len() > 64 { return Err("Binding frames exceed lexical depth limit".into()); }
            let Some(index) = active.get(item) else { return Ok(None); };
            let Value::Number(index) = index else { return Err("Invalid active binding frame".into()); };
            if !index.is_finite() || *index < 0. || index.fract() != 0. {
                return Err("Invalid active binding frame".into());
            }
            let Some(Value::Object(frame)) = frames.get(*index as usize) else { return Err("Invalid active binding frame".into()); };
            if !matches!(frame.get("__item"), Some(Value::Text(name)) if name == item) {
                return Err("Binding frame item mismatch".into());
            }
            let Some(Value::Object(parents)) = frame.get("__parents") else { return Err("Invalid binding frame parents".into()); };
            if !parents.is_empty() || matches!(frame.get("__lexicals"), Some(Value::Object(locals)) if !locals.is_empty()) {
                return resolve_binding_frame(frames, *index as usize, state, indexes, &mut BTreeMap::new(), 0);
            }
        }
        (None, None) => {} // Legacy maps without lexical frames remain readable.
        _ => return Err("Invalid lexical binding metadata".into()),
    }
    resolve_binding_item_inner(&control.state, item, state, indexes, &mut Properties::new(), 0, None)
}

// Track only direct model/member aliases. Computed collections do not acquire
// a writable origin merely because their current values resemble a model list.
fn binding_model_path(
    expression: &str, parents: &BTreeMap<String, (String, usize)>,
    aliases: &BTreeMap<String, Option<String>>, lexical_names: &Properties,
    state: &Properties,
) -> Option<String> {
    let path = expression.strip_prefix("state.").unwrap_or(expression);
    if path.is_empty() || !path.split('.').all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')) { return None; }
    let (head, tail) = path.split_once('.').map_or((path, ""), |(head, tail)| (head, tail));
    if !expression.starts_with("state.") && ["props", "tokens", "i18n"].contains(&head) { return None; }
    let append = |base: String| if tail.is_empty() { base } else { format!("{base}.{tail}") };
    if path.contains('.') && state.contains_key(path) { return Some(path.to_owned()); }
    if let Some((collection, index)) = parents.get(head) { return Some(append(format!("{collection}.{index}"))); }
    if let Some(alias) = aliases.get(head) { return alias.as_ref().map(|alias| append(alias.clone())); }
    if lexical_names.contains_key(head) { return None; }
    state.contains_key(head).then(|| path.to_owned())
}

// A direct model alias and scalar item key need no owned lexical values.
// Unsupported expressions/parents continue through the general replay below.
fn resolve_direct_alias_frame(
    frame: &Properties, state: &Properties, indexes: &mut BindingKeyIndexes, depth: usize,
) -> Option<Result<Option<(String, usize)>, String>> {
    let empty = Properties::new();
    let Value::Object(parents) = frame.get("__parents")? else { return None; };
    if !parents.is_empty() { return None; }
    let Value::Object(lexicals) = frame.get("__lexicals")? else { return None; };
    let Value::List(steps) = frame.get("__let_steps")? else { return None; };
    if steps.len() > 16_384 { return None; }
    let item = frame.get("__item")?.text().ok()?;
    let expression = frame.get(&format!("__binding_key_expr.{item}"))?;
    let scope = match frame.get(&format!("__binding_key_scope.{item}")) { Some(Value::Object(scope)) => scope, _ => &empty };
    let locals = match frame.get("__locals") { Some(Value::Object(locals)) => locals, _ => &empty };
    let member = direct_binding_key_member(expression, scope, frame, &item, state, locals)?;
    let Value::Expr(key_path) = expression else { return None; };
    if lexicals.contains_key(key_path) { return None; }
    let mut aliases = BTreeMap::new();
    for step in steps {
        let Value::Object(step) = step else { return None; };
        let name = step.get("name")?.text().ok()?;
        let Value::Expr(expression) = step.get("expression")? else { return None; };
        if matches!(step.get("parents"), Some(Value::Object(parents)) if !parents.is_empty()) { return None; }
        let scope = match step.get("scope") { Some(Value::Object(scope)) => scope, _ => &empty };
        let head = expression.split('.').next()?;
        if !expression.starts_with("state.") && (!aliases.contains_key(head) || enum_value(expression, scope).is_some()) { return None; }
        let origin = binding_model_path(expression, &BTreeMap::new(), &aliases, &empty, state)?;
        let value = lookup_binding_ref(state, &origin)?;
        if matches!(value, Value::Expr(_) | Value::Expression(_) | Value::Interpolation(_) | Value::Match(..)) { return None; }
        aliases.insert(name, Some(origin));
    }
    let collection = frame.get(&format!("__collection.{item}"))?.text().ok()?;
    let path = binding_model_path(&collection, &BTreeMap::new(), &aliases, lexicals, state)?;
    let Some(Value::List(items)) = lookup_binding_ref(state, &path) else { return None; };
    if items.len() > 16_384 { return None; }
    // Cached indexes are installed only after every key was a pure scalar read.
    // Otherwise replay is required for computed key values and their lexicals.
    if indexes.get(&path).and_then(|members| members.get(member)).is_none()
        && items.iter().any(|item| !matches!(item, Value::Object(object)
            if matches!(lookup_binding_ref(object, member), Some(Value::Text(_) | Value::Number(_)))))
    { return None; }
    Some(resolve_binding_item_inner(frame, &item, state, indexes, &mut Properties::new(), depth + 1, Some(&format!("state.{path}"))))
}

fn direct_binding_key_member<'a>(
    expression: &'a Value, scope: &Properties, metadata: &Properties,
    item: &str, state: &Properties, live_locals: &Properties,
) -> Option<&'a str> {
    let Value::Expr(expression) = expression else { return None; };
    if ["state", "props", "tokens", "i18n"].contains(&item) { return None; }
    let prefix = format!("{item}.");
    expression.strip_prefix(&prefix).filter(|tail| {
        !tail.is_empty()
            && tail.split('.').all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
            && !state.contains_key(expression)
            && !metadata.contains_key(expression)
            && !live_locals.contains_key(expression)
            && enum_value(expression, scope).is_none()
    })
}

fn resolve_binding_frame(
    frames: &[Value], index: usize, state: &Properties,
    indexes: &mut BindingKeyIndexes,
    resolved: &mut BTreeMap<usize, Option<(String, usize)>>, depth: usize,
) -> Result<Option<(String, usize)>, String> {
    if depth >= 64 { return Err("Collection binding lexical depth exceeded".into()); }
    if let Some(result) = resolved.get(&index) { return Ok(result.clone()); }
    let Some(Value::Object(frame)) = frames.get(index) else { return Err("Invalid collection binding frame".into()); };
    if let Some(result) = resolve_direct_alias_frame(frame, state, indexes, depth) {
        let result = result?;
        resolved.insert(index, result.clone());
        return Ok(result);
    }
    let item = frame.get("__item").ok_or("Missing binding frame item")?.text()?;
    let collection_name = format!("__collection.{item}");
    let Some(collection) = frame.get(&collection_name) else { return Ok(None); };
    let collection = collection.text()?;
    let empty = Properties::new();
    let parents = match frame.get("__parents") { Some(Value::Object(parents)) => parents, _ => &empty };
    let lexical_names = match frame.get("__lexicals") { Some(Value::Object(locals)) => locals, _ => &empty };
    // Replayed lexicals replace these names below. Do not deep-clone captured
    // values that would only be discarded before key resolution.
    let mut locals = match frame.get("__locals") {
        Some(Value::Object(locals)) if lexical_names.is_empty() => locals.clone(),
        Some(Value::Object(locals)) => locals.iter().filter(|(name, _)| !lexical_names.contains_key(*name))
            .map(|(name, value)| (name.clone(), value.clone())).collect(),
        _ => Properties::new(),
    };
    let mut parent_paths = BTreeMap::new();
    for (name, value) in parents {
        let parent_index = value.number()?;
        if !parent_index.is_finite() || parent_index < 0. || parent_index.fract() != 0. || parent_index as usize >= index {
            return Err("Invalid collection binding parent frame".into());
        }
        let Some((path, item_index)) = resolve_binding_frame(frames, parent_index as usize, state, indexes, resolved, depth + 1)? else { return Ok(None); };
        let Some(Value::List(items)) = lookup_binding_ref(state, &path) else { return Err("Binding parent expects a list".into()); };
        let Some(value) = items.get(item_index) else { return Ok(None); };
        if !lexical_names.contains_key(name) { locals.insert(name.clone(), value.clone()); }
        parent_paths.insert(name.clone(), (path, item_index));
    }
    let mut aliases = BTreeMap::new();
    if let Some(Value::Object(lexicals)) = frame.get("__lexicals") {
        let mut evaluated = Properties::new();
        if let Some(Value::List(steps)) = frame.get("__let_steps") {
            if steps.len() > 16_384 { return Err("Binding Let steps exceed 16384".into()); }
            let mut environment = std::borrow::Cow::Borrowed(state);
            let mut overlay_names: Vec<String> = Vec::new();
            for step in steps {
                let Value::Object(step) = step else { return Err("Invalid binding Let step".into()); };
                let name = step.get("name").ok_or("Missing binding Let name")?.text()?;
                let expression = step.get("expression").ok_or("Missing binding Let expression")?;
                let scope = match step.get("scope") { Some(Value::Object(scope)) => scope, _ => &empty };
                // The first root Let can evaluate against borrowed state. Own
                // an environment only when a previous lexical/parent is overlaid.
                if !overlay_names.is_empty() || !evaluated.is_empty() {
                    let environment = scope_mut(&mut environment);
                    for name in overlay_names.drain(..) {
                        match state.get(&name) {
                            Some(value) => { environment.set(name, value.clone()); }
                            None => { environment.unset(&name); }
                        }
                    }
                    environment.extend(evaluated.clone());
                    overlay_names.extend(evaluated.keys().cloned());
                }
                let mut step_parent_paths = BTreeMap::new();
                if let Some(Value::Object(parents)) = step.get("parents") {
                    for (name, value) in parents {
                        let parent_index = value.number()?;
                        if !parent_index.is_finite() || parent_index < 0. || parent_index.fract() != 0. || parent_index as usize >= index {
                            return Err("Invalid Let parent frame".into());
                        }
                        let Some((path, item_index)) = resolve_binding_frame(frames, parent_index as usize, state, indexes, resolved, depth + 1)? else { return Ok(None); };
                        let Some(Value::List(items)) = lookup_binding_ref(state, &path) else { return Err("Let parent expects a list".into()); };
                        let Some(value) = items.get(item_index) else { return Ok(None); };
                        scope_mut(&mut environment).set(name.clone(), value.clone());
                        overlay_names.push(name.clone());
                        step_parent_paths.insert(name.clone(), (path, item_index));
                    }
                }
                let origin = match expression {
                    Value::Expr(path) => binding_model_path(path, &step_parent_paths, &aliases, &evaluated, state),
                    _ => None,
                };
                let value = expression.evaluate(scope, &environment)?;
                aliases.insert(name.clone(), origin);
                evaluated.insert(name, value);
            }
        }
        for (name, value) in lexicals {
            locals.insert(name.clone(), evaluated.remove(name).unwrap_or_else(|| value.clone()));
        }
    }
    let Some(path) = binding_model_path(&collection, &parent_paths, &aliases, lexical_names, state) else { return Ok(None); };
    // Override only the resolved collection path; keep the retained lexical
    // frame and captured props scope borrowed, including computed key values.
    let resolved_collection = format!("state.{path}");
    let result = resolve_binding_item_inner(frame, &item, state, indexes, &mut locals, depth + 1, Some(&resolved_collection))?;
    resolved.insert(index, result.clone());
    Ok(result)
}

fn resolve_binding_item_inner(
    metadata: &Properties,
    item: &str,
    state: &Properties,
    indexes: &mut BindingKeyIndexes,
    live_locals: &mut Properties,
    depth: usize,
    collection_override: Option<&str>,
) -> Result<Option<(String, usize)>, String> {
    if depth >= 64 { return Err("Collection binding parent depth exceeded".into()); }
    let collection = match collection_override {
        Some(collection) => std::borrow::Cow::Borrowed(collection),
        None => {
            let Some(collection) = metadata.get(&format!("__collection.{item}")) else { return Ok(None); };
            std::borrow::Cow::Owned(collection.text()?)
        }
    };
    let nested_path;
    let path = if let Some(path) = collection.strip_prefix("state.") {
        path
    } else {
        let Some((parent, tail)) = collection.split_once('.') else { return Ok(None); };
        let parent_metadata = match metadata.get(&format!("__binding_parent.{item}")) {
            Some(Value::Object(parent)) => parent,
            _ => metadata,
        };
        let Some((parent_path, parent_index)) = resolve_binding_item_inner(parent_metadata, parent, state, indexes, live_locals, depth + 1, None)? else { return Ok(None); };
        let Some(Value::List(parents)) = lookup_binding_ref(state, &parent_path) else { return Ok(None); };
        let Some(parent_value) = parents.get(parent_index) else { return Ok(None); };
        live_locals.insert(parent.to_owned(), parent_value.clone());
        nested_path = format!("{parent_path}.{parent_index}.{tail}");
        &nested_path
    };
    let key = metadata
        .get(&format!("__binding_key.{item}"))
        .ok_or("Missing stable collection binding key")?
        .text()?;
    let key_expression = metadata
        .get(&format!("__binding_key_expr.{item}"))
        .ok_or("Missing collection binding key expression")?;
    let empty_scope = Properties::new();
    let key_scope = match metadata.get(&format!("__binding_key_scope.{item}")) {
        Some(Value::Object(scope)) => scope,
        _ => &empty_scope,
    };
    let Some(Value::List(items)) = lookup_binding_ref(state, path) else {
        return Err("Binding collection expects a list".into());
    };
    if items.len() > 16_384 { return Err("Binding collection exceeds 16384 items".into()); }
    // Plain item-member keys can be read directly. Preserve evaluator precedence
    // for exact dotted fields, enums, reserved namespaces and computed values.
    let direct_path = direct_binding_key_member(key_expression, key_scope, metadata, item, state, live_locals);
    if let Some(member) = direct_path {
        if let Some(keys) = indexes.get(path).and_then(|members| members.get(member)) {
            return Ok(keys.get(&key).map(|index| (path.to_owned(), *index)));
        }
    }
    let mut local = None;
    let mut keys = BTreeMap::new();
    let mut found = None;
    for (index, value) in items.iter().enumerate() {
        let direct = direct_path.and_then(|path| match value {
            Value::Object(object) => lookup_binding_ref(object, path),
            _ => None,
        }).filter(|value| matches!(value, Value::Text(_) | Value::Number(_)));
        let current_key = if let Some(value) = direct {
            key_part(value)?
        } else {
            let local = local.get_or_insert_with(|| {
                let mut local = state.scoped_clone();
                for (name, value) in metadata {
                    if name != item && !name.starts_with("__") && !local.contains_key(name) {
                        local.insert(name.clone(), value.clone());
                    }
                }
                for (name, value) in live_locals.iter() {
                    local.insert(name.clone(), value.clone());
                }
                local
            });
            local.insert(item.into(), value.clone());
            key_part(&key_expression.evaluate(key_scope, local)?)?
        };
        if current_key == key { found = Some((path.to_owned(), index)); }
        if keys.insert(current_key, index).is_some() { return Err("Duplicate collection binding key".into()); }
    }
    // Only pure member reads are independent of the retained control's scope.
    // The transaction discards these indexes after every successful model write.
    if local.is_none() {
        if let Some(member) = direct_path {
            indexes.entry(path.to_owned()).or_default().insert(member.to_owned(), keys);
        }
    }
    Ok(found)
}

fn write_collection_item_path(
    target: &mut Properties,
    col_path: &str,
    item_idx: usize,
    item_tail: &str,
    value: Value,
) -> bool {
    match lookup_binding_mut(target, col_path) {
        Some(Value::List(items)) => match items.get_mut(item_idx) {
            Some(Value::Object(item_obj)) => write_state_path(item_obj, item_tail, value),
            _ => false,
        },
        _ => false,
    }
}

fn write_state_path(target: &mut Properties, path: &str, value: Value) -> bool {
    if let Some((head, tail)) = path.split_once('.') {
        match target.get_mut(head) {
            Some(Value::Object(obj)) => write_state_path(obj, tail, value),
            Some(Value::List(items)) => write_list_path(items, tail, value),
            _ => {
                let mut nested = Properties::new();
                write_state_path(&mut nested, tail, value);
                target.insert(head.into(), Value::Object(nested));
                true
            }
        }
    } else {
        if target.get(path) == Some(&value) {
            return false;
        }
        target.insert(path.into(), value);
        true
    }
}

fn write_list_path(items: &mut [Value], path: &str, value: Value) -> bool {
    let (index, tail) = path.split_once('.').map_or((path, None), |(index, tail)| (index, Some(tail)));
    let Some(target) = index.parse::<usize>().ok().and_then(|index| items.get_mut(index)) else { return false; };
    match tail {
        None => {
            if *target == value { return false; }
            *target = value;
            true
        }
        Some(tail) => match target {
            Value::Object(object) => write_state_path(object, tail, value),
            Value::List(items) => write_list_path(items, tail, value),
            _ => false,
        },
    }
}

// Check each structural level before cloning its children. A clone-then-check
// approach can overflow the stack on malformed externally supplied trees.
fn annotated_scene_source(node: &Node, index: &mut usize, depth: usize) -> Result<Node, String> {
    if depth > 64 { return Err("Scene composition depth exceeded".into()); }
    if *index >= 16_384 { return Err("Scene exceeds 16384 nodes".into()); }
    let mut props = node.props.clone();
    props.entry("__node".into()).or_insert(number(*index as f64));
    *index += 1;
    let mut children = Vec::with_capacity(node.children.len().min(16_384 - *index));
    for child in &node.children {
        children.push(annotated_scene_source(child, index, depth + 1)?);
    }
    Ok(Node { kind: node.kind.clone(), props, children, action: node.action.clone() })
}

fn find_annotated_node<'a>(node: &'a Node, target_id: usize) -> Option<&'a Node> {
    if node.props.get("__node").and_then(|v| v.number().ok()) == Some(target_id as f64) {
        return Some(node);
    }
    for child in &node.children {
        if let Some(found) = find_annotated_node(child, target_id) {
            return Some(found);
        }
    }
    None
}

fn doc_value_to_json(v: &Value) -> Result<serde_json::Value, String> {
    Ok(match v {
        Value::Null => serde_json::Value::Null,
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::Number(n) => serde_json::Number::from_f64(*n)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        Value::Text(s) | Value::Expr(s) => serde_json::Value::String(s.clone()),
        Value::List(items) => serde_json::Value::Array(
            items
                .iter()
                .map(doc_value_to_json)
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Value::Object(map) => doc_props_to_json(map)?,
        _ => serde_json::Value::Null,
    })
}

fn doc_props_to_json(props: &Properties) -> Result<serde_json::Value, String> {
    let mut map = serde_json::Map::new();
    for (k, v) in props {
        if !k.starts_with("__") {
            map.insert(k.clone(), doc_value_to_json(v)?);
        }
    }
    Ok(serde_json::Value::Object(map))
}

fn parse_svg_attrs(raw: &str) -> BTreeMap<String, String> {
    let mut attrs = BTreeMap::new();
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let start = i;
        while i < bytes.len()
            && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-' || bytes[i] == b'_')
        {
            i += 1;
        }
        if start == i {
            i += 1;
            continue;
        }
        let key = &raw[start..i];
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'=' {
            continue;
        }
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || (bytes[i] != b'"' && bytes[i] != b'\'') {
            continue;
        }
        let quote = bytes[i];
        i += 1;
        let val_start = i;
        while i < bytes.len() && bytes[i] != quote {
            i += 1;
        }
        if i <= bytes.len() {
            attrs.insert(key.to_string(), raw[val_start..i].trim().to_string());
        }
        i += 1;
    }
    attrs
}

fn svg_num(attrs: &BTreeMap<String, String>, key: &str, fallback: f64) -> Result<f64, String> {
    let Some(raw) = attrs.get(key) else {
        return Ok(fallback);
    };
    let v: f64 = raw
        .parse()
        .map_err(|_| format!("Invalid SVG number {key}={raw}"))?;
    if !v.is_finite() {
        return Err(format!("Invalid SVG number {key}={raw}"));
    }
    Ok(v)
}

fn parse_svg_points(raw: &str) -> Result<Vec<[f64; 2]>, String> {
    let values: Vec<f64> = raw
        .split(|c: char| c.is_ascii_whitespace() || c == ',')
        .filter(|s| !s.is_empty())
        .map(|s| s.parse::<f64>().map_err(|_| "Invalid SVG points".to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    if values.len() < 4 || values.len() % 2 != 0 || values.iter().any(|n| !n.is_finite()) {
        return Err("Invalid SVG points".into());
    }
    Ok(values.chunks_exact(2).map(|c| [c[0], c[1]]).collect())
}

fn svg_ring(cx: f64, cy: f64, rx: f64, ry: f64) -> Vec<[f64; 2]> {
    (0..24)
        .map(|i| {
            let a = (i as f64 / 24.0) * std::f64::consts::TAU;
            [cx + a.cos() * rx, cy + a.sin() * ry]
        })
        .collect()
}

fn svg_stroke_segment(a: [f64; 2], b: [f64; 2], width: f64) -> Option<Vec<[f64; 2]>> {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let len = dx.hypot(dy);
    if len == 0.0 {
        return None;
    }
    let nx = (-dy / len) * width * 0.5;
    let ny = (dx / len) * width * 0.5;
    Some(vec![
        [a[0] + nx, a[1] + ny],
        [b[0] + nx, b[1] + ny],
        [b[0] - nx, b[1] - ny],
        [a[0] - nx, a[1] - ny],
    ])
}

fn parse_svg_shapes(svg: &str) -> Result<Image, String> {
    let mut root_attrs: Option<BTreeMap<String, String>> = None;
    let mut shapes: Vec<(Vec<[f64; 2]>, String)> = Vec::new();
    let mut pos = 0;
    while let Some(open) = svg[pos..].find('<') {
        let tag_start = pos + open + 1;
        let Some(close) = svg[tag_start..].find('>') else {
            break;
        };
        let inside = svg[tag_start..tag_start + close].trim();
        pos = tag_start + close + 1;
        if inside.starts_with('/') || inside.starts_with('!') || inside.starts_with('?') {
            continue;
        }
        let inside = inside.strip_suffix('/').unwrap_or(inside).trim();
        let (tag, attr_str) = match inside.find(char::is_whitespace) {
            Some(idx) => (&inside[..idx], &inside[idx..]),
            None => (inside, ""),
        };
        if tag == "g" || tag == "defs" || tag == "title" || tag == "desc" {
            continue;
        }
        let attrs = parse_svg_attrs(attr_str);
        if tag == "svg" {
            root_attrs = Some(attrs);
            continue;
        }
        let root_ref = root_attrs.as_ref().ok_or("Expected <svg> root")?;
        let fill = attrs
            .get("fill")
            .or_else(|| root_ref.get("fill"))
            .map(String::as_str)
            .unwrap_or("currentColor");
        let stroke = attrs
            .get("stroke")
            .or_else(|| root_ref.get("stroke"))
            .map(String::as_str)
            .unwrap_or("none");
        let sw = svg_num(
            &attrs,
            "stroke-width",
            svg_num(root_ref, "stroke-width", 2.0)?,
        )?;
        let points = match tag {
            "rect" => {
                let x = svg_num(&attrs, "x", 0.0)?;
                let y = svg_num(&attrs, "y", 0.0)?;
                let w = svg_num(&attrs, "width", 0.0)?;
                let h = svg_num(&attrs, "height", 0.0)?;
                vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]]
            }
            "circle" => {
                let r = svg_num(&attrs, "r", 0.0)?;
                svg_ring(svg_num(&attrs, "cx", 0.0)?, svg_num(&attrs, "cy", 0.0)?, r, r)
            }
            "ellipse" => svg_ring(
                svg_num(&attrs, "cx", 0.0)?,
                svg_num(&attrs, "cy", 0.0)?,
                svg_num(&attrs, "rx", 0.0)?,
                svg_num(&attrs, "ry", 0.0)?,
            ),
            "line" => vec![
                [svg_num(&attrs, "x1", 0.0)?, svg_num(&attrs, "y1", 0.0)?],
                [svg_num(&attrs, "x2", 0.0)?, svg_num(&attrs, "y2", 0.0)?],
            ],
            "polyline" | "polygon" => {
                parse_svg_points(attrs.get("points").map(String::as_str).unwrap_or(""))?
            }
            other => return Err(format!("Unsupported SVG element <{other}>")),
        };
        if tag != "line" && tag != "polyline" && fill != "none" {
            shapes.push((points.clone(), fill.to_string()));
        }
        if stroke != "none" && sw > 0.0 {
            let limit = if tag == "line" || tag == "polyline" {
                points.len().saturating_sub(1)
            } else {
                points.len()
            };
            for i in 0..limit {
                if let Some(quad) = svg_stroke_segment(points[i], points[(i + 1) % points.len()], sw)
                {
                    shapes.push((quad, stroke.to_string()));
                }
            }
        }
    }
    let root = root_attrs.ok_or("Expected <svg> root")?;
    let view: Vec<f64> = root
        .get("viewBox")
        .map(|s| {
            s.split(|c: char| c.is_ascii_whitespace() || c == ',')
                .filter(|p| !p.is_empty())
                .filter_map(|p| p.parse::<f64>().ok())
                .collect()
        })
        .unwrap_or_default();
    let width = if view.len() == 4 {
        view[2]
    } else {
        svg_num(&root, "width", 24.0)?
    };
    let height = if view.len() == 4 {
        view[3]
    } else {
        svg_num(&root, "height", 24.0)?
    };
    if width <= 0.0 || height <= 0.0 || shapes.is_empty() {
        return Err("SVG image has no drawable shapes".into());
    }
    Ok(Image {
        width,
        height,
        shapes,
    })
}

fn json_to_doc_value(v: &serde_json::Value) -> Result<Value, String> {
    match v {
        serde_json::Value::Null => Ok(Value::Null),
        serde_json::Value::Bool(b) => Ok(Value::Bool(*b)),
        serde_json::Value::Number(n) => Ok(number(n.as_f64().unwrap_or(0.0))),
        serde_json::Value::String(s) => Ok(Value::Text(s.clone())),
        serde_json::Value::Array(arr) => Ok(Value::List(
            arr.iter()
                .map(json_to_doc_value)
                .collect::<Result<Vec<_>, _>>()?,
        )),
        serde_json::Value::Object(obj) => {
            if let Some(serde_json::Value::String(expr)) = obj.get("expr") {
                return Ok(Value::Expr(expr.clone()));
            }
            if let Some(serde_json::Value::Array(parts)) = obj.get("interpolation") {
                return Ok(Value::Interpolation(
                    parts
                        .iter()
                        .map(json_to_doc_value)
                        .collect::<Result<Vec<_>, _>>()?,
                ));
            }
            if let Some(serde_json::Value::Object(ast)) =
                obj.get("expression").or_else(|| obj.get("ast"))
            {
                let kind = ast.get("kind").and_then(|k| k.as_str()).unwrap_or("");
                let expr = match kind {
                    "unary" => Expression::Unary(
                        ast.get("operator")
                            .or_else(|| ast.get("op"))
                            .and_then(|o| o.as_str())
                            .unwrap_or("")
                            .to_string(),
                        json_to_doc_value(
                            ast.get("argument")
                                .or_else(|| ast.get("operand"))
                                .or_else(|| ast.get("arg"))
                                .unwrap_or(&serde_json::Value::Null),
                        )?,
                    ),
                    "binary" => Expression::Binary(
                        ast.get("operator")
                            .or_else(|| ast.get("op"))
                            .and_then(|o| o.as_str())
                            .unwrap_or("")
                            .to_string(),
                        json_to_doc_value(ast.get("left").unwrap_or(&serde_json::Value::Null))?,
                        json_to_doc_value(ast.get("right").unwrap_or(&serde_json::Value::Null))?,
                    ),
                    "conditional" => Expression::Conditional(
                        json_to_doc_value(
                            ast.get("condition").unwrap_or(&serde_json::Value::Null),
                        )?,
                        json_to_doc_value(
                            ast.get("yes")
                                .or_else(|| ast.get("then"))
                                .unwrap_or(&serde_json::Value::Null),
                        )?,
                        json_to_doc_value(
                            ast.get("no")
                                .or_else(|| ast.get("else"))
                                .or_else(|| ast.get("otherwise"))
                                .unwrap_or(&serde_json::Value::Null),
                        )?,
                    ),
                    "member" => Expression::Member(
                        json_to_doc_value(ast.get("object").unwrap_or(&serde_json::Value::Null))?,
                        json_to_doc_value(ast.get("property").unwrap_or(&serde_json::Value::Null))?,
                        ast.get("optional")
                            .and_then(|b| b.as_bool())
                            .unwrap_or(false),
                    ),
                    "call" => Expression::Call(
                        ast.get("name")
                            .or_else(|| ast.get("callee"))
                            .and_then(|c| c.as_str())
                            .unwrap_or("")
                            .to_string(),
                        ast.get("arguments")
                            .or_else(|| ast.get("args"))
                            .and_then(|a| a.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .map(json_to_doc_value)
                                    .collect::<Result<Vec<_>, _>>()
                            })
                            .transpose()?
                            .unwrap_or_default(),
                    ),
                    other => return Err(format!("Unsupported AST expression kind '{other}'")),
                };
                return Ok(Value::Expression(Box::new(expr)));
            }
            if let (Some(target), Some(serde_json::Value::Array(cases))) = (
                obj.get("match"),
                obj.get("branches").or_else(|| obj.get("cases")),
            ) {
                let mut pairs = Vec::with_capacity(cases.len());
                for c in cases {
                    let pat =
                        json_to_doc_value(c.get("pattern").unwrap_or(&serde_json::Value::Null))?;
                    let val = if let Some(props) =
                        c.get("props").or_else(|| c.get("properties"))
                    {
                        json_to_doc_value(props)?
                    } else {
                        json_to_doc_value(c.get("value").unwrap_or(&serde_json::Value::Null))?
                    };
                    pairs.push((pat, val));
                }
                return Ok(Value::Match(Box::new(json_to_doc_value(target)?), pairs));
            }
            if (obj.contains_key("type") || obj.contains_key("kind"))
                && (obj.contains_key("props") || obj.contains_key("properties"))
                && obj.contains_key("children")
            {
                return Ok(Value::Node(Box::new(json_to_doc_node(v)?)));
            }
            let mut map = Properties::new();
            for (k, val) in obj {
                map.insert(k.clone(), json_to_doc_value(val)?);
            }
            Ok(Value::Object(map))
        }
    }
}

fn json_to_doc_props(v: &serde_json::Value) -> Result<Properties, String> {
    let Some(obj) = v.as_object() else {
        return Ok(Properties::new());
    };
    let mut out = Properties::new();
    for (k, val) in obj {
        out.insert(k.clone(), json_to_doc_value(val)?);
    }
    Ok(out)
}

fn json_to_doc_matches(v: Option<&serde_json::Value>) -> Result<Vec<Value>, String> {
    let Some(serde_json::Value::Array(arr)) = v else {
        return Ok(Vec::new());
    };
    arr.iter().map(json_to_doc_value).collect()
}

fn json_to_doc_node(v: &serde_json::Value) -> Result<Node, String> {
    let kind = v
        .get("type")
        .or_else(|| v.get("kind"))
        .and_then(|k| k.as_str())
        .ok_or("AST node missing type/kind")?
        .to_string();
    let mut props = json_to_doc_props(
        v.get("props")
            .or_else(|| v.get("properties"))
            .unwrap_or(&serde_json::Value::Null),
    )?;
    props.remove("slot");
    if let Some(styles_val) = v
        .get("apply")
        .or_else(|| v.get("styles"))
        .or_else(|| props.get("apply").and_then(|_| v.get("props").and_then(|p| p.get("apply"))))
    {
        props.remove("apply");
        if let Some(arr) = styles_val.as_array() {
            if !arr.is_empty() {
                props.insert(
                    "__styles".into(),
                    Value::List(
                        arr.iter()
                            .map(json_to_doc_value)
                            .collect::<Result<Vec<_>, _>>()?,
                    ),
                );
            }
        } else if let Some(s) = styles_val.as_str().or_else(|| styles_val.get("expr").and_then(|e| e.as_str())) {
            let names: Vec<Value> = s
                .split(|c: char| c == ',' || c.is_whitespace())
                .filter(|part| !part.is_empty())
                .map(|part| Value::Text(part.to_string()))
                .collect();
            if !names.is_empty() {
                props.insert("__styles".into(), Value::List(names));
            }
        }
    }
    if let Some(fwd_val) = v.get("forward") {
        if let Some(arr) = fwd_val.as_array() {
            if !arr.is_empty() {
                props.insert(
                    "__forward".into(),
                    Value::List(
                        arr.iter()
                            .map(json_to_doc_value)
                            .collect::<Result<Vec<_>, _>>()?,
                    ),
                );
            }
        }
    }
    if kind == "If" {
        if let Some(cond) = v.get("condition") {
            props.insert("condition".into(), json_to_doc_value(cond)?);
        }
    } else if kind == "For" {
        if let Some(item) = v.get("item") {
            props.insert("item".into(), json_to_doc_value(item)?);
        }
        if let Some(coll) = v.get("collection") {
            props.insert("collection".into(), json_to_doc_value(coll)?);
        }
        if let Some(key) = v.get("key") {
            props.insert("key".into(), json_to_doc_value(key)?);
        }
    } else if kind == "Let" {
        if let Some(name) = v.get("name") {
            props.insert("name".into(), json_to_doc_value(name)?);
        }
        if let Some(val) = v.get("value") {
            props.insert("value".into(), json_to_doc_value(val)?);
        }
    } else if kind == "Match" {
        if let Some(subj) = v.get("match").or_else(|| v.get("subject")) {
            props.insert("subject".into(), json_to_doc_value(subj)?);
        }
    }
    let matches = json_to_doc_matches(v.get("matches"))?;
    if !matches.is_empty() {
        props.insert("__matches".into(), Value::List(matches));
    }
    if let Some(serde_json::Value::Object(bindings_obj)) = v.get("bindings") {
        let mut bindings = Properties::new();
        for (prop_name, path_val) in bindings_obj {
            if let Some(path) = path_val
                .as_str()
                .or_else(|| path_val.get("expr").and_then(|e| e.as_str()))
            {
                props.insert(prop_name.clone(), Value::Expr(path.to_string()));
                bindings.insert(prop_name.clone(), Value::Text(path.to_string()));
            }
        }
        if !bindings.is_empty() {
            if kind == "Slider" && bindings.contains_key("value") {
                props.insert("__range_normalized".into(), Value::Bool(true));
            }
            props.insert("__bindings".into(), Value::Object(bindings));
        }
    }
    if kind == "Slider" {
        if let Some(Value::Expr(expr)) = props.get("value") {
            if !expr.ends_with('%') {
                props.insert("__range_normalized".into(), Value::Bool(true));
            }
        }
    }
    if let Some(serde_json::Value::Object(ev_args)) = v.get("eventArgs") {
        let mut event_args = Properties::new();
        for (ev_name, args_val) in ev_args {
            if let Some(args) = args_val.as_array() {
                if !args.is_empty() {
                    let converted = args
                        .iter()
                        .map(json_to_doc_value)
                        .collect::<Result<Vec<_>, _>>()?;
                    event_args.insert(ev_name.clone(), Value::List(converted));
                }
            }
        }
        if !event_args.is_empty() {
            props.insert("__event_args".into(), Value::Object(event_args));
        }
    }
    let action = v
        .get("events")
        .and_then(|e| e.get("clicked").or_else(|| e.get("dismissed")))
        .and_then(|c| c.as_str().or_else(|| c.get("action").and_then(|a| a.as_str())))
        .map(String::from);
    let mut children = v
        .get("children")
        .and_then(|c| c.as_array())
        .map(|arr| arr.iter().map(json_to_doc_node).collect::<Result<Vec<_>, _>>())
        .transpose()?
        .unwrap_or_default();
    if kind == "If" {
        if let Some(else_arr) = v.get("elseChildren").and_then(|c| c.as_array()) {
            if !else_arr.is_empty() {
                let else_nodes = else_arr
                    .iter()
                    .map(json_to_doc_node)
                    .collect::<Result<Vec<_>, _>>()?;
                children.push(Node {
                    kind: "Else".into(),
                    props: Properties::new(),
                    children: else_nodes,
                    action: None,
                });
            }
        }
    } else if kind == "For" {
        if let Some(empty_arr) = v.get("emptyChildren").and_then(|c| c.as_array()) {
            if !empty_arr.is_empty() {
                let empty_nodes = empty_arr
                    .iter()
                    .map(json_to_doc_node)
                    .collect::<Result<Vec<_>, _>>()?;
                children.push(Node {
                    kind: "Empty".into(),
                    props: Properties::new(),
                    children: empty_nodes,
                    action: None,
                });
            }
        }
    } else if kind == "Match" {
        if let Some(branches) = v.get("branches").and_then(|b| b.as_array()) {
            for br in branches {
                let mut br_props = Properties::new();
                if let Some(pat) = br.get("pattern") {
                    br_props.insert("pattern".into(), json_to_doc_value(pat)?);
                }
                if let Some(guard) = br.get("guard") {
                    if !guard.is_null() {
                        br_props.insert("guard".into(), json_to_doc_value(guard)?);
                    }
                }
                let br_children = br
                    .get("children")
                    .and_then(|c| c.as_array())
                    .map(|arr| arr.iter().map(json_to_doc_node).collect::<Result<Vec<_>, _>>())
                    .transpose()?
                    .unwrap_or_default();
                children.push(Node {
                    kind: "Branch".into(),
                    props: br_props,
                    children: br_children,
                    action: None,
                });
            }
        }
    }
    Ok(Node {
        kind,
        props,
        children,
        action,
    })
}

fn resolve_ui_file<'a>(
    files: &'a BTreeMap<String, String>,
    reference: &str,
) -> Option<(&'a str, &'a str)> {
    let trimmed = reference.trim_start_matches("./");
    for candidate in [
        reference.to_string(),
        format!("{reference}.ui"),
        format!("{reference}.component.ui"),
        trimmed.to_string(),
        format!("{trimmed}.ui"),
        format!("{trimmed}.component.ui"),
        format!("components/{trimmed}.ui"),
        format!("components/{trimmed}.component.ui"),
    ] {
        if let Some((k, v)) = files.get_key_value(&candidate) {
            return Some((k.as_str(), v.as_str()));
        }
    }
    None
}

fn merge_json_object(target: &mut serde_json::Map<String, serde_json::Value>, source: &serde_json::Value) {
    if let Some(obj) = source.as_object() {
        for (k, v) in obj {
            target.insert(k.clone(), v.clone());
        }
    }
}

fn compile_single_component_ast(
    files: &BTreeMap<String, String>,
    file_name: &str,
    stack: &mut Vec<String>,
) -> Result<serde_json::Value, String> {
    if stack.contains(&file_name.to_string()) {
        return Err(format!(
            "Component inheritance cycle: {} -> {file_name}",
            stack.join(" -> ")
        ));
    }
    let (_, source) = resolve_ui_file(files, file_name)
        .ok_or_else(|| format!("Missing component file '{file_name}'"))?;
    let mut ast = themoretheless_tokenizer_forma::parse(source).map_err(|e| e.to_string())?;
    stack.push(file_name.to_string());

    // 1. Resolve `uses` imports (enums, structs, structConstraints, tokens, i18n, styles)
    let uses = ast
        .get("uses")
        .and_then(|u| u.as_array())
        .cloned()
        .unwrap_or_default();
    for use_ref in uses {
        if let Some(use_name) = use_ref.as_str() {
            let imported = compile_single_component_ast(files, use_name, stack)?;
            for section in [
                "enums",
                "structs",
                "structConstraints",
                "tokens",
                "i18n",
                "styles",
            ] {
                let mut combined = imported
                    .get(section)
                    .and_then(|s| s.as_object())
                    .cloned()
                    .unwrap_or_default();
                if let Some(own) = ast.get(section) {
                    merge_json_object(&mut combined, own);
                }
                ast[section] = serde_json::Value::Object(combined);
            }
        }
    }

    // 2. Resolve `base` component inheritance and `slots` overrides
    if let Some(base_ref) = ast.get("base").and_then(|b| b.as_str()).map(String::from) {
        let base_ast = compile_single_component_ast(files, &base_ref, stack)?;
        for section in [
            "defaults",
            "propDefinitions",
            "stateDefinitions",
            "eventDefinitions",
            "enums",
            "structs",
            "structConstraints",
            "tokens",
            "i18n",
            "styles",
            "lets",
            "slotDefinitions",
        ] {
            let mut combined = base_ast
                .get(section)
                .and_then(|s| s.as_object())
                .cloned()
                .unwrap_or_default();
            if let Some(own) = ast.get(section) {
                merge_json_object(&mut combined, own);
            }
            ast[section] = serde_json::Value::Object(combined);
        }
        let mut combined_matches = base_ast
            .get("matches")
            .and_then(|m| m.as_array())
            .cloned()
            .unwrap_or_default();
        if let Some(own_matches) = ast.get("matches").and_then(|m| m.as_array()) {
            combined_matches.extend(own_matches.iter().cloned());
        }
        ast["matches"] = serde_json::Value::Array(combined_matches);

        let own_nodes = ast
            .get("nodes")
            .and_then(|n| n.as_array())
            .cloned()
            .unwrap_or_default();
        if own_nodes.is_empty() {
            let mut base_nodes = base_ast
                .get("nodes")
                .and_then(|n| n.as_array())
                .cloned()
                .unwrap_or_default();
            if let Some(slots) = ast.get("slots").and_then(|s| s.as_object()) {
                apply_slot_overrides(&mut base_nodes, slots, files)?;
            }
            ast["nodes"] = serde_json::Value::Array(base_nodes);
        }
    }

    stack.pop();
    Ok(ast)
}

fn substitute_base_slot(
    target: &mut serde_json::Value,
    slot_name: &str,
    original_node: &serde_json::Value,
) {
    if let Some(children) = target.get_mut("children").and_then(|c| c.as_array_mut()) {
        for child in children.iter_mut() {
            let is_base_ref = child
                .get("type")
                .and_then(|t| t.as_str())
                .is_some_and(|t| t == format!("base.{slot_name}"));
            if is_base_ref {
                *child = original_node.clone();
            } else {
                substitute_base_slot(child, slot_name, original_node);
            }
        }
    }
}

fn apply_slot_overrides(
    nodes: &mut Vec<serde_json::Value>,
    slots: &serde_json::Map<String, serde_json::Value>,
    files: &BTreeMap<String, String>,
) -> Result<(), String> {
    for node in nodes.iter_mut() {
        let slot_name = node
            .get("props")
            .or_else(|| node.get("properties"))
            .and_then(|p| p.get("slot").or_else(|| p.get("key")))
            .and_then(|s| s.as_str().or_else(|| s.get("expr").and_then(|e| e.as_str())))
            .map(String::from);
        if let Some(slot) = slot_name {
            if let Some(override_spec) = slots.get(&slot) {
                let original = node.clone();
                if override_spec
                    .get("propertyOverride")
                    .and_then(|b| b.as_bool())
                    .unwrap_or(false)
                {
                    let patch = override_spec.get("patch").unwrap_or(override_spec);
                    if let Some(props) = node.get_mut("props").and_then(|p| p.as_object_mut()) {
                        merge_json_object(
                            props,
                            patch
                                .get("props")
                                .or_else(|| patch.get("properties"))
                                .unwrap_or(&serde_json::Value::Null),
                        );
                    }
                    if let Some(ov_matches) = patch.get("matches").and_then(|m| m.as_array()) {
                        let mut list = node
                            .get("matches")
                            .and_then(|m| m.as_array())
                            .cloned()
                            .unwrap_or_default();
                        list.extend(ov_matches.iter().cloned());
                        node["matches"] = serde_json::Value::Array(list);
                    }
                } else if override_spec
                    .get("fileOverride")
                    .and_then(|b| b.as_bool())
                    .unwrap_or(false)
                    || override_spec.get("fileOverride").is_some_and(|v| v.is_string())
                {
                    let file_override = override_spec
                        .get("file")
                        .or_else(|| override_spec.get("fileOverride"))
                        .and_then(|f| f.as_str())
                        .unwrap_or("");
                    let (_, frag_src) = resolve_ui_file(files, file_override)
                        .ok_or_else(|| format!("Missing slot override file '{file_override}'"))?;
                    let frag = themoretheless_tokenizer_forma::parse_with_options(frag_src, true)
                        .map_err(|e| e.to_string())?;
                    if let Some(frag_nodes) = frag.get("nodes").and_then(|n| n.as_array()) {
                        if frag_nodes.len() == 1 {
                            let mut replacement = frag_nodes[0].clone();
                            substitute_base_slot(&mut replacement, &slot, &original);
                            *node = replacement;
                        } else {
                            node["children"] = serde_json::Value::Array(frag_nodes.clone());
                        }
                    }
                } else if override_spec.get("type").is_some() {
                    let mut replacement = override_spec.clone();
                    substitute_base_slot(&mut replacement, &slot, &original);
                    if replacement
                        .get("props")
                        .and_then(|p| p.get("slot"))
                        .is_none()
                    {
                        if let Some(props) =
                            replacement.get_mut("props").and_then(|p| p.as_object_mut())
                        {
                            props.insert("slot".into(), serde_json::Value::String(slot));
                        }
                    }
                    *node = replacement;
                } else if let Some(ov_nodes) = override_spec.get("nodes").and_then(|n| n.as_array())
                {
                    if ov_nodes.len() == 1 {
                        let mut replacement = ov_nodes[0].clone();
                        substitute_base_slot(&mut replacement, &slot, &original);
                        if replacement
                            .get("props")
                            .and_then(|p| p.get("slot"))
                            .is_none()
                        {
                            if let Some(props) =
                                replacement.get_mut("props").and_then(|p| p.as_object_mut())
                            {
                                props.insert("slot".into(), serde_json::Value::String(slot));
                            }
                        }
                        *node = replacement;
                    }
                }
            }
        }
        if let Some(children) = node.get_mut("children").and_then(|c| c.as_array_mut()) {
            apply_slot_overrides(children, slots, files)?;
        }
    }
    Ok(())
}

fn ast_to_definition(ast: &serde_json::Value) -> Result<Definition, String> {
    let mut defaults = json_to_doc_props(ast.get("defaults").unwrap_or(&serde_json::Value::Null))?;
    for (ast_key, meta_key) in [
        ("propDefinitions", "__contracts"),
        ("enums", "__enums"),
        ("structs", "__structs"),
        ("structConstraints", "__struct_constraints"),
        ("tokens", "__tokens"),
        ("i18n", "__i18n"),
        ("styles", "__styles"),
        ("lets", "__lets"),
    ] {
        let map = json_to_doc_props(ast.get(ast_key).unwrap_or(&serde_json::Value::Null))?;
        if !map.is_empty() {
            defaults.insert(meta_key.into(), Value::Object(map));
        }
    }
    let matches = json_to_doc_matches(ast.get("matches"))?;
    if !matches.is_empty() {
        defaults.insert("__matches".into(), Value::List(matches));
    }
    let nodes = ast
        .get("nodes")
        .and_then(|n| n.as_array())
        .map(|arr| arr.iter().map(json_to_doc_node).collect::<Result<Vec<_>, _>>())
        .transpose()?
        .unwrap_or_default();
    Ok(Definition { defaults, nodes })
}

fn collect_component_names(node: &Node, out: &mut std::collections::BTreeSet<String>) {
    if ![
        "Frame",
        "Scroll",
        "Row",
        "Column",
        "Grid",
        "Stack",
        "If",
        "Else",
        "For",
        "Empty",
        "Modal",
        "Rectangle",
        "Text",
        "TextInput",
        "Image",
        "PointerArea",
        "RangeInput",
        "ContentPresenter",
        "Let",
        "Match",
        "Branch",
    ]
    .contains(&node.kind.as_str())
    {
        out.insert(node.kind.clone());
    }
    for child in &node.children {
        collect_component_names(child, out);
    }
}

fn compile_ui_files_to_document(
    files: &BTreeMap<String, String>,
    entry_file: &str,
    initial_state: &Properties,
) -> Result<Document, String> {
    let mut stack = Vec::new();
    let scene_ast = compile_single_component_ast(files, entry_file, &mut stack)?;
    let name = scene_ast
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("Scene")
        .to_string();
    let mut scene_nodes = scene_ast
        .get("nodes")
        .and_then(|n| n.as_array())
        .map(|arr| arr.iter().map(json_to_doc_node).collect::<Result<Vec<_>, _>>())
        .transpose()?
        .unwrap_or_default();
    if scene_nodes.len() != 1 {
        return Err("Scene component must have a single layout root node".into());
    }
    let mut root = scene_nodes.remove(0);

    // Attach scene-level scope (`defaults`, `tokens`, `i18n`, `styles`, `enums`, `structs`, `lets`, `matches`)
    let scene_def = ast_to_definition(&scene_ast)?;
    if !scene_def.defaults.is_empty() {
        root.props
            .insert("__scope".into(), Value::Object(scene_def.defaults));
    }

    // Parse all SVG assets in `files`
    let mut images = BTreeMap::new();
    for (path, content) in files {
        if path.ends_with(".svg") {
            images.insert(path.clone(), parse_svg_shapes(content)?);
        }
    }

    // Discover and compile all referenced component definitions transitively
    let mut definitions = BTreeMap::new();
    let mut pending = std::collections::BTreeSet::new();
    collect_component_names(&root, &mut pending);
    for (path, _) in files {
        if path.ends_with(".component.ui") {
            if let Some(stem) = path.strip_suffix(".component.ui") {
                pending.insert(stem.to_string());
            }
        }
    }
    let mut visited = std::collections::BTreeSet::new();
    while let Some(comp_name) = pending.pop_first() {
        if !visited.insert(comp_name.clone()) {
            continue;
        }
        if resolve_ui_file(files, &comp_name).is_some() {
            let comp_ast = compile_single_component_ast(files, &comp_name, &mut Vec::new())?;
            let def = ast_to_definition(&comp_ast)?;
            for n in &def.nodes {
                collect_component_names(n, &mut pending);
            }
            definitions.insert(comp_name, def);
        }
    }
    for (builtin_name, builtin_src) in [
        (
            "Text",
            "component Text { width: 260; height: 30; text: ''; fontSize: 16; color: #ffffff; wrap: false; background: #00000000; borderWidth: 0; radius: 0; Rectangle { background: props.background; Text { text: props.text; color: props.color; fontSize: props.fontSize; wrap: props.wrap; } } }",
        ),
        (
            "TextInput",
            "component TextInput { width: 240; height: 38; value: ''; label: ''; placeholder: ''; multiline: false; wrap: false; readOnly: false; disabled: false; text: props.value; fontSize: 15; color: #ffffff; placeholderColor: #999999; background: #00000000; borderWidth: 0; radius: 0; Rectangle { background: props.background; TextInput { forward props { value, label, placeholder, multiline, wrap, readOnly, fontSize, color, placeholderColor }; } PointerArea { clicked -> events.clicked(); } } }",
        ),
        (
            "Slider",
            "component Slider { width: 240; height: 32; value: 0%; disabled: false; background: #00000000; borderWidth: 0; radius: 0; Rectangle { background: props.background; PointerArea { clicked -> events.clicked(); } } }",
        ),
    ] {
        if !definitions.contains_key(builtin_name) {
            let comp_ast = themoretheless_tokenizer_forma::parse(builtin_src).map_err(|e| e.to_string())?;
            definitions.insert(builtin_name.into(), ast_to_definition(&comp_ast)?);
        }
    }

    // Merge initial state from `stateDefinitions` defaults + `designs` + `initial_state`
    let mut root_state = Properties::new();
    if let Some(state_defs) = scene_ast.get("stateDefinitions").and_then(|s| s.as_object()) {
        for (k, spec) in state_defs {
            if let Some(def_val) = spec.get("default").or_else(|| spec.get("defaultValue")) {
                if !def_val.is_null() {
                    root_state.insert(k.clone(), json_to_doc_value(def_val)?);
                }
            }
        }
    }
    if let Some(designs) = scene_ast.get("designs").and_then(|d| d.as_array()) {
        for design_ref in designs {
            if let Some(design_file) = design_ref.as_str() {
                if let Some((_, design_src)) = resolve_ui_file(files, design_file) {
                    let design_ast = themoretheless_tokenizer_forma::parse(design_src)
                        .map_err(|e| e.to_string())?;
                    let patch = themoretheless_tokenizer_forma::semantics::design_state_patch(
                        &design_ast,
                        None,
                    )
                    .map_err(|e| e.to_string())?;
                    if let Some(obj) = patch.as_object() {
                        for (k, v) in obj {
                            root_state.insert(k.clone(), json_to_doc_value(v)?);
                        }
                    }
                }
            }
        }
    }
    for (k, v) in initial_state {
        root_state.insert(k.clone(), v.clone());
    }

    Ok(Document {
        name,
        root,
        definitions,
        fallback: Properties::new(),
        images,
        states: vec![root_state],
    })
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
    fn sibling_control_snapshots_share_root_until_mutated() {
        let files = BTreeMap::from([("App.ui".into(), "component App { state query: String = 'seed'; state payload: List = []; Column { TextInput { key:'a'; value <-> state.query; } TextInput { key:'b'; value <-> state.query; } } }".into())]);
        let payload = Value::List((0..256).map(|i| string(&format!("payload-{i}"))).collect());
        let mut doc = Document::from_ui_files(&files,"App.ui",&props(vec![("payload",payload)])).unwrap();
        let (_,_,mut controls) = doc.render_scene().unwrap();
        assert_eq!(controls.len(),2);
        assert!(controls[0].state.shares_storage(&controls[1].state),"siblings must share the expanded root map");
        let before = serde_json::to_string(&controls[1]).unwrap();
        controls[0].state.insert("query".into(),string("snapshot-edit"));
        doc.states[0].insert("query".into(),string("model-edit"));
        assert_eq!(serde_json::to_string(&controls[1]).unwrap(),before);
        assert_eq!(controls[0].state["query"],string("snapshot-edit"));
        assert_eq!(doc.states[0]["query"],string("model-edit"));
    }

    #[test]
    fn shared_property_snapshots_detach_on_every_mutable_access() {
        let mut original = props(vec![("object", Value::Object(props(vec![("caption", string("before"))]))), ("list", Value::List(vec![string("first")]))]);
        let snapshot = original.clone();
        assert!(original.shares_storage(&snapshot));
        let expected_json = serde_json::to_string(&snapshot).unwrap();
        let Some(Value::Object(object)) = original.get_mut("object") else { panic!("object") };
        object.insert("caption".into(), string("after"));
        let Some(Value::List(list)) = original.get_mut("list") else { panic!("list") };
        list.push(string("second"));
        assert!(!original.shares_storage(&snapshot));
        assert_eq!(serde_json::to_string(&snapshot).unwrap(), expected_json);
        assert_ne!(original, snapshot);
        let restored: Properties = serde_json::from_str(&expected_json).unwrap();
        assert_eq!(restored, snapshot);
        let mut detached = snapshot.clone();
        for (_, value) in &mut detached { *value = Value::Null; }
        assert_eq!(serde_json::to_string(&snapshot).unwrap(), expected_json);
        let consumed: Properties = snapshot.clone().into_iter().collect();
        assert_eq!(consumed, snapshot);
    }

    #[test]
    fn scene_default_lookup_preserves_overlay_and_missing_state_evaluation() {
        let mut doc = document(root(vec![]));
        doc.fallback.insert("seed".into(), string("fallback"));
        doc.fallback.insert("text".into(), expr("props.seed"));
        let definition = doc.definitions.get_mut("Control").unwrap();
        definition.defaults.insert("seed".into(), string("definition"));
        definition.defaults.insert("text".into(), expr("props.seed"));
        let instance = node("Control", vec![("text", expr("state.absent"))], vec![]);
        let resolved = doc.scene_properties(&instance, &Properties::new(), &Properties::new()).unwrap();
        assert_eq!(resolved["text"], string("definition"));
        let plain = node("Text", vec![("text", expr("state.absent"))], vec![]);
        assert_eq!(doc.scene_properties(&plain, &Properties::new(), &Properties::new()).unwrap()["text"], string("fallback"));
        let state = props(vec![("absent", string("live"))]);
        assert_eq!(doc.scene_properties(&instance, &Properties::new(), &state).unwrap()["text"], string("live"));
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
        let (_, explicit) = doc.instantiate(&node("Control", vec![
            ("flag", Value::Bool(false)), ("text", string("explicit")),
        ], vec![]), true, &Properties::new(), &[]).unwrap();
        assert_eq!(explicit["flag"], Value::Bool(false));
        assert_eq!(explicit["text"], string("explicit"));
        let (_, metadata) = doc.instantiate(&node("Control", vec![
            ("flag", Value::Bool(false)), ("text", expr("props.__state.caption")),
            ("__state", Value::Object(props(vec![("caption", string("snapshot"))]))),
        ], vec![]), true, &Properties::new(), &[]).unwrap();
        assert_eq!(metadata["text"], string("snapshot"));
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
    fn collection_binding_activation_follows_key_after_reorder_or_removal() {
        for (remove, cached) in [(false, false), (true, false), (false, true), (true, true)] {
            let mut doc = document(root(vec![node("For", vec![
                ("item", string("item")), ("collection", expr("state.items")), ("key", expr("item.id")),
            ], vec![node("Control", vec![
                ("text", expr("item.title")),
                ("__bindings", Value::Object(props(vec![("checked", string("item.done"))]))),
            ], vec![])])]));
            let item = |id: &str| Value::Object(props(vec![
                ("id", string(id)), ("title", string(id)), ("done", Value::Bool(false)),
            ]));
            doc.states = vec![props(vec![("items", Value::List(vec![item("a"), item("b")]))])];
            let (source, templates, controls) = doc.render_scene().unwrap();
            let runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
            let items = if remove { vec![item("b")] } else { vec![item("b"), item("a")] };
            doc.states[0].insert("items".into(), Value::List(items));
            let changed = if cached { doc.sync_runtime_bindings_from_scene(&runtime, Some(0), &controls).unwrap() }
                else { doc.sync_runtime_bindings(&runtime, Some(0)).unwrap() };
            assert_eq!(changed, if remove { 0 } else { 1 });
            let repeated = doc.sync_runtime_binding_events(&runtime, &[0, 0], &controls).unwrap();
            assert_eq!(repeated, if remove { 0 } else { 2 });
            let Value::List(items) = &doc.states[0]["items"] else { panic!("items"); };
            let Value::Object(b) = &items[0] else { panic!("b"); };
            assert_eq!(b["done"], Value::Bool(false), "the old first index must not target b");
            if !remove {
                let Value::Object(a) = &items[1] else { panic!("a"); };
                assert_eq!(a["done"], Value::Bool(true));
            }
        }
    }
    #[test]
    fn collection_editor_values_follow_keys_and_do_not_overwrite_new_items() {
        for (mode, cached) in (0..4).flat_map(|mode| [(mode, false), (mode, true)]) {
            let mut doc = document(root(vec![node("For", vec![
                ("item", string("item")), ("collection", expr("state.items")), ("key", expr("item.id")),
            ], vec![node("Control", vec![
                ("value", expr("item.title")),
                ("__bindings", Value::Object(props(vec![("value", string("item.title"))]))),
            ], vec![])])]));
            let definition = doc.definitions.get_mut("Control").unwrap();
            definition.defaults.insert("value".into(), string(""));
            definition.nodes = vec![node("Rectangle", vec![], vec![node("TextInput", vec![("value", expr("props.value"))], vec![])])];
            doc.states = vec![props(vec![("items", Value::List(vec![record("a", "Alpha"), record("b", "Beta")]))])];
            let (source, templates, controls) = doc.render_scene().unwrap();
            let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
            runtime.set_binding_value(0, "edited-a", true).unwrap();
            let items = match mode {
                0 => vec![record("b", "Beta"), record("a", "Alpha")],
                1 => vec![record("b", "Beta")],
                2 => vec![record("c", "New"), record("b", "model-b"), record("a", "Alpha")],
                _ => vec![record("b", "Beta"), record("b", "Duplicate")],
            };
            doc.states[0].insert("items".into(), Value::List(items));
            let before = doc.states.clone();
            let result = if cached { doc.sync_runtime_bindings_from_scene(&runtime, None, &controls) }
                else { doc.sync_runtime_bindings(&runtime, None) };
            if mode == 3 {
                assert!(result.unwrap_err().contains("Duplicate collection"));
                assert_eq!(doc.states, before);
                continue;
            }
            result.unwrap();
            let Value::List(items) = &doc.states[0]["items"] else { panic!("items"); };
            for item in items {
                let Value::Object(item) = item else { panic!("record"); };
                let expected = match item["id"].text().unwrap().as_str() { "a" => "edited-a", "b" => if mode == 2 { "model-b" } else { "Beta" }, "c" => "New", _ => unreachable!() };
                assert_eq!(item["title"], string(expected));
            }
        }
    }
    #[test]
    fn ambiguous_scene_and_runtime_binding_keys_refuse_before_writing() {
        let files = BTreeMap::from([("App.ui".to_string(),
            "component App { state a: String = 'one'; state b: String = 'two'; Column { width:260; height:100; TextInput { key:'a'; width:200; height:32; value <-> state.a; } TextInput { key:'b'; width:200; height:32; value <-> state.b; } } }".to_string())]);
        let mut doc = Document::from_ui_files(&files, "App.ui", &Properties::new()).unwrap();
        let (source, templates, controls) = doc.render_scene().unwrap();
        let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
        runtime.set_binding_value(0, "edited", true).unwrap();
        let before = doc.states.clone();
        let mut ambiguous = controls.clone();
        ambiguous[1].key = ambiguous[0].key.clone();
        assert!(doc.sync_runtime_bindings_from_scene(&runtime, None, &ambiguous).unwrap_err().contains("Ambiguous binding"));
        assert_eq!(doc.states, before);
        let source = source.replace("key:'b'", "key:'a'");
        assert!(crate::Runtime::from_sources(&source, &templates).err().unwrap().contains("Duplicate control key"));
        assert_eq!(doc.states, before);
    }
    #[test]
    fn unchanged_widgets_do_not_overwrite_newer_model_values() {
        let files = BTreeMap::from([("App.ui".to_string(),
            "component App { state a: String = 'one'; state b: String = 'two'; Column { width:260; height:100; TextInput { width:200; height:32; value <-> state.a; } TextInput { width:200; height:32; value <-> state.b; } } }".to_string())]);
        let mut doc = Document::from_ui_files(&files, "App.ui", &Properties::new()).unwrap();
        let (source, templates, _) = doc.render_scene().unwrap();
        let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
        runtime.set_binding_value(0, "edited", true).unwrap();
        doc.states[0].insert("a".into(), string("concurrent-model-a"));
        doc.states[0].insert("b".into(), string("new-model-b"));
        assert_eq!(doc.sync_runtime_bindings(&runtime, None).unwrap(), 1);
        assert_eq!(doc.states[0]["a"], string("edited"));
        assert_eq!(doc.states[0]["b"], string("new-model-b"));
        // A committed widget must not replay its value in a later model update.
        doc.states[0].insert("a".into(), string("new-model-a"));
        assert_eq!(doc.sync_runtime_bindings(&runtime, None).unwrap(), 0);
        assert_eq!(doc.states[0]["a"], string("new-model-a"));
    }
    #[test]
    fn unchanged_range_preserves_model_and_committed_input_does_not_replay() {
        let mut doc = document(root(vec![node("Control", vec![
            ("value", expr("state.level")),
            ("__bindings", Value::Object(props(vec![("value", string("state.level"))]))),
        ], vec![])]));
        doc.states = vec![props(vec![("level", number(0.5))])];
        let definition = doc.definitions.get_mut("Control").unwrap();
        definition.defaults.insert("value".into(), number(0.5));
        let range = node("RangeInput", vec![("value", expr("props.value"))], vec![
            node("Minimum", vec![], vec![node("ContentShape", vec![("points", string("0 0 10 0 10 10")), ("color", string("#ffffff"))], vec![])]),
            node("Maximum", vec![], vec![node("ContentShape", vec![("points", string("0 0 80 0 80 10")), ("color", string("#ffffff"))], vec![])]),
        ]);
        definition.nodes[0].children.remove(0);
        definition.nodes[0].children.insert(0, range);
        let (source, templates, _) = doc.render_scene().unwrap();
        let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
        doc.states[0].insert("level".into(), number(0.8));
        assert_eq!(doc.sync_runtime_bindings(&runtime, None).unwrap(), 0);
        assert_eq!(doc.states[0]["level"], number(0.8));
        assert!(runtime.focus_control(0));
        assert!(runtime.range_key("ArrowRight"));
        let edited = runtime.range_value(0) as f64;
        assert_eq!(doc.sync_runtime_bindings(&runtime, None).unwrap(), 1);
        assert_eq!(doc.states[0]["level"], number(edited));
        doc.states[0].insert("level".into(), number(0.7));
        assert_eq!(doc.sync_runtime_bindings(&runtime, None).unwrap(), 0);
        assert_eq!(doc.states[0]["level"], number(0.7));
    }
    #[test]
    fn collection_key_changes_revalidate_following_bindings() {
        let files = BTreeMap::from([("App.ui".to_string(),
            "component App { state items: List = []; Column { width:260; height:100; for item in state.items key item.id { TextInput { width:200; height:32; value <-> item.id; } } } }".to_string())]);
        let state = props(vec![("items", Value::List(vec![
            Value::Object(props(vec![("id", string("a"))])),
            Value::Object(props(vec![("id", string("b"))])),
        ]))]);
        let mut doc = Document::from_ui_files(&files, "App.ui", &state).unwrap();
        let (source, templates, _) = doc.render_scene().unwrap();
        let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
        let before = doc.states.clone();
        runtime.set_binding_value(0, "b", true).unwrap();
        assert!(doc.sync_runtime_bindings(&runtime, None).unwrap_err().contains("Duplicate collection"));
        assert_eq!(doc.states, before, "later lookup must not use the pre-write key index");
        runtime.set_binding_value(0, "a-new", true).unwrap();
        assert_eq!(doc.sync_runtime_bindings(&runtime, None).unwrap(), 1);
        let Value::List(items) = &doc.states[0]["items"] else { panic!("items"); };
        let Value::Object(first) = &items[0] else { panic!("item"); };
        assert_eq!(first["id"], string("a-new"));
    }
    #[test]
    fn collection_indices_handle_distinct_outer_local_scopes() {
        for groups in [2, 10] {
        let files = BTreeMap::from([("App.ui".to_string(),
            "component App { state items: List = []; state groups: List = []; Column { width:260; height:220; for outer in state.groups key outer.id { for item in state.items key outer.id + item.id { TextInput { width:200; height:32; value <-> item.title; } } } } }".to_string())]);
        let state = props(vec![
            ("items", Value::List(vec![record("a", "Alpha"), record("b", "Beta")])),
            ("groups", Value::List((0..groups).map(|i| record(&format!("g{i}"), "Group")).collect())),
        ]);
        let mut doc = Document::from_ui_files(&files, "App.ui", &state).unwrap();
        let (source, templates, controls) = doc.render_scene().unwrap();
        let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
        assert_eq!(runtime.control_count(), groups * 2);
        runtime.set_binding_value(groups * 2 - 1, "edited-b", true).unwrap();
        assert_eq!(doc.sync_runtime_bindings(&runtime, None).unwrap(), 1);
        let Value::List(items) = &doc.states[0]["items"] else { panic!("items"); };
        let Value::Object(second) = &items[1] else { panic!("item"); };
        assert_eq!(second["title"], string("edited-b"));
        if groups == 2 {
            doc.states[0].insert("items".into(), Value::List(vec![record("x", "Oversized"); 16_385]));
            assert!(resolve_binding_item(&controls[0], "item", &doc.states[0]).unwrap_err().contains("16384 items"));
        }
        }
    }
    #[test]
    fn unchanged_virtual_collection_member_does_not_become_a_model_write() {
        let files = BTreeMap::from([("App.ui".to_string(),
            "component App { state items: List = []; Column { width:260; height:100; for item in state.items key item.id { TextInput { width:200; height:32; value:'5'; } } } }".to_string())]);
        let state = props(vec![("items", Value::List(vec![record("a", "Alpha")]))]);
        let mut doc = Document::from_ui_files(&files, "App.ui", &state).unwrap();
        doc.root.children[0].children[0].props.insert("__bindings".into(), Value::Object(props(vec![("value", string("item.title.length"))])));
        let (source, templates, _) = doc.render_scene().unwrap();
        let runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
        let before = doc.states.clone();
        assert_eq!(doc.sync_runtime_bindings(&runtime, None).unwrap(), 0);
        assert_eq!(doc.states, before);
    }
    #[test]
    fn let_collection_aliases_preserve_origin_and_do_not_infer_it_from_values() {
        for collection in ["chosen", "state.chosen"] {
        for derived in [false, true] {
        for cached in [false, true] {
            let initial_items = Value::List(vec![record("a", "Alpha"), record("b", "Beta")]);
            let mut doc = document(root(vec![
                node("Let", vec![("name", string("rows")), ("value", if derived { initial_items.clone() } else { expr("state.items") })], vec![]),
                node("Let", vec![("name", string("chosen")), ("value", expr("rows"))], vec![]),
                node("For", vec![("item", string("item")), ("collection", expr(collection)), ("key", expr("item.id"))], vec![
                    node("Control", vec![("value", expr("item.title")), ("__bindings", Value::Object(props(vec![("value", string("item.title"))])))], vec![]),
                ]),
            ]));
            let definition = doc.definitions.get_mut("Control").unwrap();
            definition.defaults.insert("value".into(), string(""));
            definition.nodes = vec![node("Rectangle", vec![], vec![node("TextInput", vec![("value", expr("props.value"))], vec![])])];
            doc.states = vec![props(vec![("items", initial_items), ("chosen", Value::List(vec![record("other", "Model shadow")]))])];
            let (source, templates, controls) = doc.render_scene().unwrap();
            let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
            runtime.set_binding_value(0, "Edited", true).unwrap();
            let Value::List(items) = doc.states[0].get_mut("items").unwrap() else { panic!("items") };
            items.reverse();
            let changed = if cached { doc.sync_runtime_bindings_from_scene(&runtime, None, &controls) }
                else { doc.sync_runtime_bindings(&runtime, None) };
            assert_eq!(changed.unwrap(), usize::from(!derived));
            assert_eq!(lookup(&doc.states[0], "items.1.title"), Some(string(if derived { "Alpha" } else { "Edited" })));
            assert_eq!(lookup(&doc.states[0], "items.0.title"), Some(string("Beta")));
            assert_eq!(lookup(&doc.states[0], "chosen.0.title"), Some(string("Model shadow")));
        }
        }
        }
    }
    #[test]
    fn failed_let_replay_rolls_back_earlier_widget_writes_and_retries() {
        for cached in [false, true] {
            let editor = |value: Value, path: &str| node("Control", vec![
                ("value", value), ("__bindings", Value::Object(props(vec![("value", string(path))]))),
            ], vec![]);
            let mut doc = document(root(vec![
                editor(expr("state.query"), "state.query"),
                node("Let", vec![("name", string("prefix")), ("value", expr("state.prefix"))], vec![]),
                node("For", vec![("item", string("item")), ("collection", expr("state.items")),
                    ("key", Value::Expression(Box::new(Expression::Binary("+".into(), expr("prefix"), expr("item.id")))))],
                    vec![editor(expr("item.title"), "item.title")]),
            ]));
            let definition = doc.definitions.get_mut("Control").unwrap();
            definition.defaults.insert("value".into(), string(""));
            definition.nodes = vec![node("Rectangle", vec![], vec![node("TextInput", vec![("value", expr("props.value"))], vec![])])];
            doc.states = vec![props(vec![("query", string("seed")), ("prefix", string("p/")),
                ("items", Value::List(vec![record("a", "Alpha")]))])];
            let (source, templates, controls) = doc.render_scene().unwrap();
            let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
            runtime.set_binding_value(0, "Edited query", true).unwrap();
            runtime.set_binding_value(1, "Edited item", true).unwrap();
            doc.states[0].remove("prefix");
            let before = doc.states.clone();
            for _ in 0..2 {
                let result = if cached { doc.sync_runtime_bindings_from_scene(&runtime, None, &controls) }
                    else { doc.sync_runtime_bindings(&runtime, None) };
                assert!(result.unwrap_err().contains("Missing state.prefix"));
                assert_eq!(doc.states, before, "earlier editor write must roll back");
            }
            doc.states[0].insert("prefix".into(), string("p/"));
            let mut corrupt = controls.clone();
            let Some(Value::Object(active)) = corrupt[1].state.get_mut("__binding_active") else { panic!("active frames") };
            active.insert("item".into(), number(-1.));
            let before = doc.states.clone();
            assert!(doc.sync_runtime_bindings_from_scene(&runtime, None, &corrupt).unwrap_err().contains("Invalid active binding frame"));
            assert_eq!(doc.states, before, "invalid frame must roll back the earlier editor write");
            assert_eq!(doc.sync_runtime_bindings_from_scene(&runtime, None, &controls).unwrap(), 2);
            assert_eq!(doc.states[0]["query"], string("Edited query"));
            assert_eq!(lookup(&doc.states[0], "items.0.title"), Some(string("Edited item")));
            assert_eq!(doc.sync_runtime_bindings_from_scene(&runtime, None, &controls).unwrap(), 0);
            doc.states[0].insert("query".into(), string("Model query"));
            assert_eq!(doc.sync_runtime_bindings_from_scene(&runtime, None, &controls).unwrap(), 0);
            assert_eq!(doc.states[0]["query"], string("Model query"));
        }
    }
    #[test]
    fn let_key_local_shadows_root_model_during_binding_recovery() {
        for dynamic in [false, true] {
        for cached in [false, true] {
            let mut doc = document(root(vec![
                node("Let", vec![("name", string("prefix")), ("value", if dynamic { expr("state.prefix") } else { string("lexical/") })], vec![]),
                node("Let", vec![("name", string("suffix")), ("value", expr("prefix"))], vec![]),
                node("For", vec![("item", string("item")), ("collection", expr("state.items")), ("key", Value::Expression(Box::new(Expression::Binary("+".into(), expr("suffix"), expr("item.id")))))], vec![
                    node("Control", vec![("value", expr("item.title")), ("__bindings", Value::Object(props(vec![("value", string("item.title"))])))], vec![]),
                ]),
            ]));
            let definition = doc.definitions.get_mut("Control").unwrap();
            definition.defaults.insert("value".into(), string(""));
            definition.nodes = vec![node("Rectangle", vec![], vec![node("TextInput", vec![("value", expr("props.value"))], vec![])])];
            doc.states = vec![props(vec![("prefix", string("model/")), ("items", Value::List(vec![record("a", "Alpha"), record("b", "Beta")]))])];
            let (source, templates, controls) = doc.render_scene().unwrap();
            let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
            runtime.set_binding_value(0, "Edited", true).unwrap();
            doc.states[0].insert("prefix".into(), string("new-model/"));
            let Value::List(items) = doc.states[0].get_mut("items").unwrap() else { panic!("items") };
            items.reverse();
            let changed = if cached { doc.sync_runtime_bindings_from_scene(&runtime, None, &controls) }
                else { doc.sync_runtime_bindings(&runtime, None) };
            assert_eq!(changed.unwrap(), usize::from(!dynamic));
            assert_eq!(lookup(&doc.states[0], "items.1.title"), Some(string(if dynamic { "Alpha" } else { "Edited" })));
            assert_eq!(doc.states[0]["prefix"], string("new-model/"));
        }
        }
    }
    #[test]
    fn lexical_parent_frames_must_point_to_earlier_frames() {
        let frames = vec![Value::Object(props(vec![
            ("__item", string("item")),
            ("__collection.item", string("state.items")),
            ("__parents", Value::Object(props(vec![("item", number(0.))]))),
        ]))];
        assert!(resolve_binding_frame(&frames, 0, &Properties::new(), &mut BindingKeyIndexes::new(), &mut BTreeMap::new(), 0)
            .unwrap_err().contains("parent frame"));
    }
    #[test]
    fn later_loop_can_use_earlier_local_after_an_unrelated_shadow() {
        for cached in [false, true] {
            let files = BTreeMap::from([("App.ui".into(),
                "component App { state groups: List = []; state other: List = []; Column { width:260; height:400; for item in state.groups key item.id { for branch in item.items key branch.id { for item in state.other key item.id { for leaf in branch.items key item.id + leaf.id { TextInput { width:200; height:32; value <-> leaf.title; } } } } } } }".into())]);
            let group = |id: &str, children| Value::Object(props(vec![("id", string(id)), ("items", Value::List(children))]));
            let state = props(vec![
                ("groups", Value::List(vec![
                    group("g1", vec![group("branch", vec![record("leaf", "First")])]),
                    group("g2", vec![group("branch", vec![record("leaf", "Second")])]),
                ])),
                ("other", Value::List(vec![record("inner", "Unused")])),
            ]);
            let mut doc = Document::from_ui_files(&files, "App.ui", &state).unwrap();
            let (source, templates, controls) = doc.render_scene().unwrap();
            let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
            runtime.set_binding_value(0, "Edited First", true).unwrap();
            let Value::List(groups) = doc.states[0].get_mut("groups").unwrap() else { panic!("groups") };
            groups.reverse();
            let changed = if cached { doc.sync_runtime_bindings_from_scene(&runtime, None, &controls) }
                else { doc.sync_runtime_bindings(&runtime, None) };
            assert_eq!(changed.unwrap(), 1);
            assert_eq!(lookup(&doc.states[0], "groups.1.items.0.items.0.title"), Some(string("Edited First")));
            assert_eq!(lookup(&doc.states[0], "groups.0.items.0.items.0.title"), Some(string("Second")));
        }
    }
    #[test]
    fn three_shadowed_for_levels_preserve_parent_identity() {
        for middle in ["item", "branch"] {
        for cached in [false, true] {
            let source = "component App { state groups: List = []; Column { width:260; height:400; for item in state.groups key item.id { for middle in item.items key middle.id { for item in middle.items key item.id { TextInput { width:200; height:32; value <-> item.title; } } } } } }".replace("middle", middle);
            let files = BTreeMap::from([("App.ui".into(), source)]);
            let group = |id: &str, children| Value::Object(props(vec![("id", string(id)), ("items", Value::List(children))]));
            let state = props(vec![("groups", Value::List(vec![
                group("root", vec![group("mid", vec![record("leaf", "Before")])]),
                group("other", vec![group("mid", vec![record("leaf", "Unchanged")])]),
            ]))]);
            let mut doc = Document::from_ui_files(&files, "App.ui", &state).unwrap();
            let (source, templates, controls) = doc.render_scene().unwrap();
            let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
            runtime.set_binding_value(0, "After", true).unwrap();
            let Value::List(groups) = doc.states[0].get_mut("groups").unwrap() else { panic!("groups") };
            groups.reverse();
            let changed = if cached { doc.sync_runtime_bindings_from_scene(&runtime, None, &controls) }
                else { doc.sync_runtime_bindings(&runtime, None) };
            assert_eq!(changed.unwrap(), 1);
            assert_eq!(lookup(&doc.states[0], "groups.1.items.0.items.0.title"), Some(string("After")));
            assert_eq!(lookup(&doc.states[0], "groups.0.items.0.items.0.title"), Some(string("Unchanged")));
        }
        }
    }
    #[test]
    fn cyclic_collection_parent_metadata_refuses_before_recursing_unbounded() {
        let control = SceneControl {
            node: 0, key: String::new(), event_args: BTreeMap::new(),
            state: props(vec![("__collection.item", string("item.items"))]),
        };
        assert!(resolve_binding_item(&control, "item", &Properties::new()).unwrap_err().contains("parent depth"));
    }
    #[test]
    fn source_scroll_viewport_extents_preserve_frame_and_clip_input() {
        for (width, height, padding, expected) in [
            ("320", "300", "0", [20., 50., 320., 300.]),
            ("50%", "50%", "0", [90., 100., 180., 200.]),
            ("50%", "50%", "20", [100., 110., 160., 180.]),
        ] {
            let source = format!("component App {{ state items: List = []; Column {{ width:360; height:400; padding:{padding}; Scroll {{ width:{width}; height:{height}; Column {{ width:100; height:512; for item in state.items key item.id {{ TextInput {{ width:100; height:32; value <-> item.title; }} }} }} }} }} }}");
            let files = BTreeMap::from([("App.ui".into(), source)]);
            let state = props(vec![("items", Value::List((0..16).map(|i| record(&format!("id{i}"), "Row")).collect()))]);
            let mut runtime = crate::Runtime::from_ui_files(&files, "App.ui", &state).unwrap();
            assert!(runtime.scrollable());
            assert_eq!(runtime.viewport(), expected.map(|v| v as f32).to_vec());
            assert_eq!(runtime.render_width(), 360.);
            assert_eq!(runtime.render_height(), 400.);
            runtime.scroll(0., 10_000.);
            assert_eq!(runtime.scroll_offset()[1], 512. - expected[3] as f32);
            runtime.pointer(expected[0] as f32 + expected[2] as f32 / 2., expected[1] as f32 - 1., 0);
            assert!((0..16).all(|i| !runtime.control_hovered(i)));
            runtime.pointer(expected[0] as f32 + expected[2] as f32 / 2., expected[1] as f32 + 10., 0);
            assert!((0..16).any(|i| runtime.control_hovered(i)));
        }
        let files = BTreeMap::from([("App.ui".into(),
            "component App { Column { width:360; height:400; Scroll { width:500; TextInput { width:100; height:32; } } } }".into())]);
        let doc = Document::from_ui_files(&files, "App.ui", &Properties::new()).unwrap();
        assert!(doc.render_scene().unwrap_err().contains("viewport must fit"));
    }
    #[test]
    fn empty_scroll_layout_retains_logical_content_height() {
        for (content, expected) in [(512, 212.), (900, 600.), (200, 0.)] {
            let source = format!("component App {{ Column {{ width:360; height:400; Scroll {{ width:320; height:300; Column {{ width:100; height:{content}; }} }} }} }}");
            let files = BTreeMap::from([("App.ui".into(), source)]);
            let mut runtime = crate::Runtime::from_ui_files(&files, "App.ui", &Properties::new()).unwrap();
            assert_eq!(runtime.control_count(), 0);
            runtime.scroll(0., 10_000.);
            assert_eq!(runtime.scroll_offset()[1], expected);
            assert_eq!(runtime.viewport(), vec![20., 50., 320., 300.]);
        }
    }

    #[test]
    fn structural_binding_maps_match_rendered_maps_across_layouts_and_branches() {
        let body = "let prefix = 'p/'; if state.enabled { for item in state.items key prefix + item.id { TextInput { width:100; height:32; value <-> item.title; } } } else { TextInput { width:100; height:32; value <-> state.query; } } Text { text:state.marker; }";
        let mut layouts: Vec<String> = ["Frame", "Column", "Row", "Grid", "Stack"].iter()
            .map(|kind| format!("{kind} {{ width:360; height:400; {body} }}")).collect();
        layouts.push(format!("Column {{ width:360; height:400; Scroll {{ width:360; height:300; Column {{ width:320; height:600; {body} }} }} }}"));
        for layout in layouts {
            let source = format!("component App {{ state enabled: Bool = true; state query: String = 'seed'; state marker: String = 'snapshot'; state items: List = []; {layout} }}");
            let files = BTreeMap::from([("App.ui".into(), source)]);
            let state = props(vec![("items", Value::List(vec![record("a", "Alpha"), record("b", "Beta")]))]);
            let mut doc = Document::from_ui_files(&files, "App.ui", &state).unwrap();
            fn annotate_events(node: &mut Node) {
                if node.kind == "TextInput" {
                    node.props.insert("__event_args".into(), Value::Object(props(vec![
                        ("snapshot", Value::List(vec![expr("state.marker")]))
                    ])));
                }
                for child in &mut node.children { annotate_events(child); }
            }
            annotate_events(&mut doc.root);
            for enabled in [true, false, true] {
                doc.states[0].insert("enabled".into(), Value::Bool(enabled));
                let Value::List(items) = doc.states[0].get_mut("items").unwrap() else { panic!("items") };
                items.reverse();
                let (source, templates, rendered) = doc.render_scene().unwrap();
                crate::Runtime::from_sources(&source, &templates).unwrap();
                let structural = doc.binding_scene_controls().unwrap();
                assert_eq!(structural.len(), rendered.len());
                for (actual, expected) in structural.iter().zip(&rendered) {
                    assert_eq!(actual.node, expected.node);
                    assert_eq!(actual.key, expected.key);
                    assert_eq!(actual.state, expected.state);
                    assert_eq!(actual.event_args, expected.event_args);
                }
                assert_eq!(structural[0].event_args["snapshot"], vec![string("snapshot")]);
            }
        }
    }
    #[test]
    fn source_match_let_aliases_and_guard_switch_preserve_binding_identity() {
        let files = BTreeMap::from([("App.ui".into(),
            "component App { state mode: String = 'a'; state enabled: Bool = true; state prefix: String = 'root'; state other: String = 'Other'; state items: List = []; Column { width:260; height:300; match state.mode { 'a' if state.enabled => { let prefix = 'branch/'; let rows = state.items; for item in rows key prefix + item.id { TextInput { width:200; height:32; value <-> item.title; } } } _ => { let prefix = 'fallback'; let suffix = prefix; TextInput { width:200; height:32; value <-> state.other; } } } Text { text:state.prefix; } } }".into())]);
        let state = props(vec![("items", Value::List(vec![record("a", "Alpha")]))]);
        let mut doc = Document::from_ui_files(&files, "App.ui", &state).unwrap();
        let (source, templates, first) = doc.render_scene().unwrap();
        assert_eq!(first[0].state["prefix"], string("branch/"));
        assert_eq!(first[1].state["prefix"], string("root"));
        let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
        runtime.set_binding_value(0, "Old branch input", true).unwrap();
        doc.states[0].insert("enabled".into(), Value::Bool(false));
        assert_eq!(doc.sync_runtime_bindings(&runtime, None).unwrap(), 0);
        assert_eq!(lookup(&doc.states[0], "items.0.title"), Some(string("Alpha")));
        assert_eq!(doc.states[0]["other"], string("Other"));
        let (source, templates, fallback) = doc.render_scene().unwrap();
        assert_ne!(first[0].key, fallback[0].key);
        assert_eq!(fallback[0].state["suffix"], string("fallback"));
        assert!(!fallback[1].state.contains_key("suffix"));
        let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
        runtime.set_binding_value(0, "Edited fallback", true).unwrap();
        assert_eq!(doc.sync_runtime_bindings(&runtime, None).unwrap(), 1);
        assert_eq!(doc.states[0]["other"], string("Edited fallback"));
        doc.states[0].insert("enabled".into(), Value::Bool(true));
        assert_eq!(doc.sync_runtime_bindings(&runtime, None).unwrap(), 0);
        let (_, _, again) = doc.render_scene().unwrap();
        assert_eq!(again[0].key, first[0].key);
        assert_eq!(doc.states[0]["prefix"], string("root"));
    }
    #[test]
    fn source_if_let_sequence_is_visible_only_in_selected_branch() {
        let files = BTreeMap::from([("App.ui".into(),
            "component App { state prefix: String = 'root'; state enabled: Bool = true; Column { width:260; height:300; if state.enabled { let prefix = 'yes'; let suffix = prefix; Text { text:suffix; } } else { let prefix = 'no'; let suffix = prefix; Text { text:suffix; } } Text { text:state.prefix; } } }".into())]);
        let mut doc = Document::from_ui_files(&files, "App.ui", &Properties::new()).unwrap();
        for (enabled, expected) in [(true, "yes"), (false, "no"), (true, "yes")] {
            doc.states[0].insert("enabled".into(), Value::Bool(enabled));
            let (source, templates, controls) = doc.render_scene().unwrap();
            crate::Runtime::from_sources(&source, &templates).unwrap();
            assert_eq!(controls.len(), 2);
            assert_eq!(controls[0].state["suffix"], string(expected));
            assert_eq!(controls[1].state["prefix"], string("root"));
            assert!(!controls[1].state.contains_key("suffix"));
            assert_eq!(doc.states[0]["prefix"], string("root"));
        }
    }
    #[test]
    fn source_for_let_siblings_are_visible_and_do_not_leak_between_iterations() {
        let files = BTreeMap::from([("App.ui".into(),
            "component App { state prefix: String = 'root/'; state items: List = []; Column { width:260; height:300; for item in state.items key state.prefix + item.id { let prefix = item.id; TextInput { width:200; height:32; value <-> item.title; } } Text { text:state.prefix; } } }".into())]);
        let state = props(vec![("items", Value::List(vec![record("a", "Alpha"), record("b", "Beta")]))]);
        let mut doc = Document::from_ui_files(&files, "App.ui", &state).unwrap();
        let (_, _, first) = doc.render_scene().unwrap();
        assert_eq!(first[0].state["prefix"], string("a"));
        assert_eq!(first[1].state["prefix"], string("b"));
        assert_eq!(first[2].state["prefix"], string("root/"));
        let Value::List(items) = doc.states[0].get_mut("items").unwrap() else { panic!("items") };
        items.reverse();
        let (_, _, second) = doc.render_scene().unwrap();
        assert_eq!(first[0].key, second[1].key);
        assert_eq!(first[1].key, second[0].key);
        assert_eq!(doc.states[0]["prefix"], string("root/"));
    }
    #[test]
    fn nested_for_bindings_follow_parent_and_child_keys() {
        for alias in [false, true] {
        for shadow in [false, true] {
        for cached in [false, true] {
            for mode in [0, 1, 2] {
                let mut files: BTreeMap<String, String> = BTreeMap::from([("App.ui".into(),
                    "component App { state groups: List = []; Column { width:260; height:400; for outer in state.groups key outer.id { for item in outer.items key item.id { TextInput { width:200; height:32; value <-> item.title; } } } } }".into())]);
                if shadow {
                    let source = files.get_mut("App.ui").unwrap();
                    *source = source.replace("outer", "item");
                }
                if alias {
                    let source = files.get_mut("App.ui").unwrap();
                    let parent = if shadow { "item" } else { "outer" };
                    *source = source.replace(&format!("for item in {parent}.items"), &format!("let rows = {parent}.items; for item in rows"));
                }
                let group = |id: &str, children| Value::Object(props(vec![("id", string(id)), ("items", Value::List(children))]));
                let initial = props(vec![("groups", Value::List(vec![
                    group("g1", vec![record("a", "Alpha"), record("b", "Beta")]),
                    group("g2", vec![record("a", "Other Alpha")]),
                ]))]);
                let mut doc = Document::from_ui_files(&files, "App.ui", &initial).unwrap();
                let (source, templates, controls) = doc.render_scene().unwrap();
                let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
                runtime.set_binding_value(0, "edited-g1-a", true).unwrap();
                let groups = match mode {
                    0 => vec![group("g2", vec![record("a", "Other Alpha")]), group("g1", vec![record("b", "Beta"), record("a", "Alpha")])],
                    1 => vec![group("g2", vec![record("a", "Other Alpha")])],
                    _ => vec![group("g1", vec![record("a", "Alpha")]), group("g1", vec![record("a", "Duplicate")])],
                };
                doc.states[0].insert("groups".into(), Value::List(groups));
                let before = doc.states.clone();
                let result = if cached { doc.sync_runtime_bindings_from_scene(&runtime, None, &controls) }
                    else { doc.sync_runtime_bindings(&runtime, None) };
                if mode == 2 {
                    assert!(result.unwrap_err().contains("Duplicate collection"));
                    assert_eq!(doc.states, before);
                } else {
                    assert_eq!(result.unwrap(), usize::from(mode == 0));
                    assert_eq!(lookup(&doc.states[0], "groups.0.items.0.title"), Some(string("Other Alpha")));
                    if mode == 0 {
                        assert_eq!(lookup(&doc.states[0], "groups.1.items.1.title"), Some(string("edited-g1-a")));
                        assert_eq!(lookup(&doc.states[0], "groups.1.items.0.title"), Some(string("Beta")));
                    }
                }
            }
        }
        }
        }
    }
    #[test]
    fn nested_list_paths_read_and_write_without_replacing_lists() {
        let mut state = props(vec![("groups", Value::List(vec![Value::Object(props(vec![
            ("items", Value::List(vec![record("a", "Alpha"), record("b", "Beta")]))
        ]))]))]);
        assert_eq!(lookup(&state, "groups.0.items.1.title"), Some(string("Beta")));
        assert_eq!(lookup_binding_ref(&state, "groups.0.items.1.title"), Some(&string("Beta")));
        assert_eq!(lookup(&state, "groups.0.items.length"), Some(number(2.)));
        assert!(lookup_binding_ref(&state, "groups.0.items.length").is_none());
        assert!(write_collection_item_path(&mut state, "groups.0.items", 1, "title", string("Edited")));
        assert_eq!(lookup(&state, "groups.0.items.1.title"), Some(string("Edited")));
        assert!(write_state_path(&mut state, "groups.0.items.0.title", string("First")));
        let before = state.clone();
        for path in ["groups.9.items.0.title", "groups.bad.items", "groups.0.items.length"] {
            assert!(!write_state_path(&mut state, path, string("invalid")));
            assert_eq!(state, before);
        }
    }
    #[test]
    fn bindings_route_root_backed_collections_through_list_indices() {
        let mut doc = document(root(vec![node("For", vec![
            ("item", string("item")), ("collection", expr("state.groups.0.items")),
            ("key", expr("item.id")),
        ], vec![node("Control", vec![
            ("value", expr("item.title")),
            ("__bindings", Value::Object(props(vec![("value", string("item.title"))]))),
        ], vec![])])]));
        let definition = doc.definitions.get_mut("Control").unwrap();
        definition.defaults.insert("value".into(), string(""));
        definition.nodes = vec![node("Rectangle", vec![], vec![node("TextInput", vec![("value", expr("props.value"))], vec![])])];
        doc.states = vec![props(vec![("groups", Value::List(vec![Value::Object(props(vec![
            ("items", Value::List(vec![record("a", "Alpha"), record("b", "Beta")]))
        ]))]))])];
        let (source, templates, controls) = doc.render_scene().unwrap();
        let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
        runtime.set_binding_value(0, "edited-a", true).unwrap();
        let Some(Value::List(items)) = lookup_binding_mut(&mut doc.states[0], "groups.0.items") else { panic!("items") };
        items.reverse();
        assert_eq!(doc.sync_runtime_bindings_from_scene(&runtime, None, &controls).unwrap(), 1);
        assert_eq!(lookup(&doc.states[0], "groups.0.items.1.title"), Some(string("edited-a")));
        assert_eq!(lookup(&doc.states[0], "groups.0.items.0.title"), Some(string("Beta")));
    }
    #[test]
    fn collection_key_encoding_preserves_types_and_utf8_lengths() {
        for (value, expected) in [
            (string(""), "s0:"), (string("я🙂"), "s6:я🙂"),
            (string("12"), "s2:12"), (number(12.), "n2:12"),
            (number(-1.5), "n4:-1.5"),
        ] {
            assert_eq!(key_part(&value).unwrap(), expected);
        }
        for value in [number(f64::NAN), number(f64::INFINITY), Value::Bool(true), Value::Null] {
            assert!(key_part(&value).is_err());
        }
    }
    #[test]
    fn borrowed_collection_keys_preserve_computed_and_exact_field_semantics() {
        let item = |id: Value, raw: &str| Value::Object(props(vec![("id", id), ("raw", string(raw))]));
        let mut state = props(vec![("items", Value::List(vec![
            item(string("a"), "unused"), item(expr("item.raw"), "b"),
        ]))]);
        let control = SceneControl {
            node: 0, key: String::new(), event_args: BTreeMap::new(),
            state: props(vec![
                ("__collection.item", string("state.items")),
                ("__binding_key.item", string(&key_part(&string("b")).unwrap())),
                ("__binding_key_expr.item", expr("item.id")),
            ]),
        };
        assert_eq!(resolve_binding_item(&control, "item", &state).unwrap(), Some(("items".into(), 1)));
        // Exact dotted root fields take precedence over item members, causing
        // duplicate keys here; the borrowed path must not bypass this refusal.
        state.insert("item.id".into(), string("override"));
        assert!(resolve_binding_item(&control, "item", &state).unwrap_err().contains("Duplicate"));
        state.remove("item.id");
        let Value::List(items) = state.get_mut("items").unwrap() else { panic!("items") };
        items.reverse();
        assert_eq!(resolve_binding_item(&control, "item", &state).unwrap(), Some(("items".into(), 0)));
    }
    #[test]
    fn reused_for_scope_preserves_nested_shadowing_and_owned_snapshots() {
        let control = || node("Control", vec![("text", expr("item.title"))], vec![]);
        let loop_node = |collection: &str, children| node("For", vec![
            ("item", string("item")), ("collection", expr(collection)),
            ("key", expr("item.id")),
        ], children);
        let mut doc = document(root(vec![loop_node("state.items", vec![
            loop_node("state.inner", vec![control()]), control(),
        ])]));
        doc.states = vec![props(vec![
            ("items", Value::List(vec![record("a", "Alpha"), record("b", "Beta")])),
            ("inner", Value::List(vec![record("x", "Ex"), record("y", "Why")])),
        ])];
        let (_, _, snapshots) = doc.render_scene().unwrap();
        let titles: Vec<_> = snapshots.iter().map(|c| {
            let Value::Object(item) = &c.state["item"] else { panic!("item snapshot") };
            item["title"].text().unwrap()
        }).collect();
        assert_eq!(titles, ["Ex", "Why", "Alpha", "Ex", "Why", "Beta"]);
        for (i, key) in ["x", "y", "a", "x", "y", "b"].iter().enumerate() {
            assert_eq!(snapshots[i].state["__binding_key.item"], string(&key_part(&string(key)).unwrap()));
        }
        doc.states[0].insert("items".into(), Value::List(vec![record("c", "Changed")]));
        let (_, _, next) = doc.render_scene().unwrap();
        assert_eq!(next.len(), 3);
        assert_eq!(snapshots[5].state["__binding_key.item"], string(&key_part(&string("b")).unwrap()));
        assert!(!doc.states[0].contains_key("item"));
    }
    #[test]
    fn borrowed_root_alias_keys_match_general_replay_on_reorder_and_key_errors() {
        let files = BTreeMap::from([("App.ui".into(), "component App { state items: List = []; Column { let visible = state.items; let rows = visible; for item in rows key item.id { TextInput { width:100; height:30; value <-> item.title; } } } }".into())]);
        let item = |id: Value, title: &str| Value::Object(props(vec![("id", id), ("title", string(title))]));
        let mut doc = Document::from_ui_files(&files, "App.ui", &props(vec![("items", Value::List(vec![item(string("a"), "Alpha"), item(string("b"), "Beta")]))])).unwrap();
        let (_, _, controls) = doc.render_scene().unwrap();
        let mut reference = controls[0].clone();
        let Value::List(frames) = reference.state.get_mut("__binding_frames").unwrap() else { panic!("frames") };
        let Value::Object(frame) = &mut frames[0] else { panic!("frame") };
        let Value::List(steps) = frame.get_mut("__let_steps").unwrap() else { panic!("steps") };
        // An unrelated literal Let requires general replay without changing the key.
        steps.push(Value::Object(props(vec![("name", string("unused_marker")), ("expression", string("marker"))])));
        let Value::Object(lexicals) = frame.get_mut("__lexicals").unwrap() else { panic!("lexicals") };
        lexicals.insert("unused_marker".into(), string("marker"));
        let compare = |state: &Properties| {
            let actual = resolve_binding_item(&controls[0], "item", state);
            let expected = resolve_binding_item(&reference, "item", state);
            assert_eq!(actual, expected);
            actual
        };
        assert_eq!(compare(&doc.states[0]).unwrap(), Some(("items".into(), 0)));
        let Value::List(items) = doc.states[0].get_mut("items").unwrap() else { panic!("items") };
        items.reverse();
        assert_eq!(compare(&doc.states[0]).unwrap(), Some(("items".into(), 1)));
        doc.states[0].insert("item.id".into(), string("override"));
        assert!(compare(&doc.states[0]).unwrap_err().contains("Duplicate"));
        doc.states[0].remove("item.id");
        let Value::List(items) = doc.states[0].get_mut("items").unwrap() else { panic!("items") };
        let Value::Object(first) = &mut items[0] else { panic!("item") };
        first.insert("id".into(), expr("item.title"));
        assert_eq!(compare(&doc.states[0]).unwrap(), Some(("items".into(), 1)));
    }

    #[test]
    fn direct_alias_replay_preserves_shadowing_and_readonly_origins() {
        for (lets, target) in [
            ("let visible = state.items;", Some("items")),
            ("let rows = state.items; let visible = rows;", Some("items")),
            ("let items = state.other; let visible = items;", Some("other")),
            ("let visible = state.items; let items = state.other;", Some("items")),
            ("let visible = state.items; let visible = state.other;", Some("other")),
            ("let marker = 'literal'; let visible = state.items;", Some("items")),
            ("let visible = true ? state.items : state.other;", None),
        ] {
            let source = format!("component App {{ state items: List = []; state other: List = []; Column {{ {lets} for item in visible key item.id {{ TextInput {{ width:100; height:30; value <-> item.title; }} }} }} }}");
            let files = BTreeMap::from([("App.ui".into(), source)]);
            let list = |prefix: &str| Value::List(["a", "b"].into_iter().map(|id| Value::Object(props(vec![("id", string(id)), ("title", string(&format!("{prefix}-{id}")))]))).collect());
            let mut doc = Document::from_ui_files(&files, "App.ui", &props(vec![("items", list("items")), ("other", list("other"))])).unwrap();
            let (_, _, controls) = doc.render_scene().unwrap();
            let mut reference = controls[0].clone();
            let Value::List(frames) = reference.state.get_mut("__binding_frames").unwrap() else { panic!("frames") };
            let Value::Object(frame) = &mut frames[0] else { panic!("frame") };
            let Value::List(steps) = frame.get_mut("__let_steps").unwrap() else { panic!("steps") };
            steps.push(Value::Object(props(vec![("name", string("unused_reference_marker")), ("expression", string("marker"))])));
            let Value::Object(lexicals) = frame.get_mut("__lexicals").unwrap() else { panic!("lexicals") };
            lexicals.insert("unused_reference_marker".into(), string("marker"));
            for reversed in [false, true] {
                if reversed { for name in ["items", "other"] {
                    let Value::List(items) = doc.states[0].get_mut(name).unwrap() else { panic!("items") }; items.reverse();
                } }
                let actual = resolve_binding_item(&controls[0], "item", &doc.states[0]);
                let expected = resolve_binding_item(&reference, "item", &doc.states[0]);
                assert_eq!(actual, expected, "{lets}");
                assert_eq!(actual.unwrap(), target.map(|target| (target.into(), usize::from(reversed))), "{lets}");
            }
        }
    }

    #[test]
    fn direct_alias_shortcut_does_not_hide_an_unused_let_replay_error() {
        let files = BTreeMap::from([("App.ui".into(), "component App { state items: List = []; state unused: List = []; Column { let ignored = state.unused; let visible = state.items; for item in visible key item.id { TextInput { width:100; height:30; value <-> item.title; } } } }".into())]);
        let mut doc = Document::from_ui_files(&files, "App.ui", &props(vec![("items", Value::List(vec![Value::Object(props(vec![("id", string("a")), ("title", string("Alpha"))]))]))])).unwrap();
        let (_, _, controls) = doc.render_scene().unwrap();
        doc.states[0].remove("unused");
        let before = doc.states.clone();
        assert!(resolve_binding_item(&controls[0], "item", &doc.states[0]).unwrap_err().contains("Missing state.unused"));
        assert_eq!(doc.states, before);
    }

    #[test]
    fn later_let_metadata_is_retained_when_not_represented_in_collection_frame() {
        let files = BTreeMap::from([("App.ui".into(), "component App { state items: List = []; state suffix: String = 'tail'; Column { let visible = state.items; for item in visible key item.id { let extra = state.suffix; TextInput { width:100; height:30; value <-> item.title; } } } }".into())]);
        let state = props(vec![("items", Value::List(vec![Value::Object(props(vec![("id", string("a")), ("title", string("Alpha"))]))]))]);
        let doc = Document::from_ui_files(&files, "App.ui", &state).unwrap();
        let (_, _, controls) = doc.render_scene().unwrap();
        assert_eq!(controls.len(), 1);
        assert_eq!(controls[0].state["extra"], string("tail"));
        let Value::Object(lexicals) = &controls[0].state["__binding_lexicals"] else { panic!("lexicals") };
        assert_eq!(lexicals["extra"], string("tail"));
        assert!(controls[0].state.contains_key("__binding_let_steps"));
    }
    #[test]
    fn compact_lexical_frames_preserve_model_values_and_legacy_map_recovery() {
        let files = BTreeMap::from([("App.ui".into(), "component App { state items: List = []; Column { let visible = state.items; for item in visible key item.id { TextInput { width:100; height:30; value <-> item.title; } } } }".into())]);
        let item = |id: &str, title: &str| Value::Object(props(vec![("id", string(id)), ("title", string(title))]));
        let mut doc = Document::from_ui_files(&files, "App.ui", &props(vec![("items", Value::List(vec![item("a", "Alpha"), item("b", "Beta")]))])).unwrap();
        let mut legacy_doc = doc.clone();
        let (source, templates, controls) = doc.render_scene().unwrap();
        let mut legacy = controls.clone();
        for control in &mut legacy {
            let Value::List(frames) = control.state.get_mut("__binding_frames").unwrap() else { panic!("frames") };
            let Value::Object(frame) = &mut frames[0] else { panic!("frame") };
            let Value::Object(lexicals) = frame["__lexicals"].clone() else { panic!("lexicals") };
            let Value::Object(locals) = frame.get_mut("__locals").unwrap() else { panic!("locals") };
            assert!(!locals.contains_key("visible"));
            locals.extend(lexicals);
            let replay_lexicals = frame["__lexicals"].clone();
            let replay_steps = frame["__let_steps"].clone();
            control.state.insert("__binding_lexicals".into(), replay_lexicals);
            control.state.insert("__binding_let_steps".into(), replay_steps);
        }
        assert!(!controls[0].state.contains_key("__binding_lexicals"));
        assert!(!controls[0].state.contains_key("__binding_let_steps"));
        assert!(matches!(&controls[0].state["visible"], Value::List(items) if items.len() == 2));
        assert!(matches!(&controls[0].state["items"], Value::List(items) if items.len() == 2));
        let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
        runtime.set_binding_value(0, "edited", true).unwrap();
        let mut legacy_runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
        legacy_runtime.set_binding_value(0, "edited", true).unwrap();
        assert_eq!(doc.sync_runtime_bindings_from_scene(&runtime, None, &controls).unwrap(), 1);
        assert_eq!(legacy_doc.sync_runtime_bindings_from_scene(&legacy_runtime, None, &legacy).unwrap(), 1);
        assert_eq!(doc.states, legacy_doc.states);
    }
    #[test]
    fn computed_item_member_keys_keep_captured_props_scope_during_recovery() {
        let files = BTreeMap::from([("App.ui".into(), "component App { prop prefix: String = 'stable'; state items: List = []; Column { let visible = state.items; let chosen = visible; for item in chosen key item.id { TextInput { width:100; height:30; value <-> item.title; } } } }".into())]);
        let item = |id: Value, title: &str| Value::Object(props(vec![("id", id), ("title", string(title))]));
        let mut doc = Document::from_ui_files(&files, "App.ui", &props(vec![("items", Value::List(vec![item(expr("props.prefix"), "Alpha"), item(string("other"), "Beta")]))])).unwrap();
        let (source, templates, controls) = doc.render_scene().unwrap();
        let frames = controls[0].state["__binding_frames"].clone();
        let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
        runtime.set_binding_value(0, "edited", true).unwrap();
        let Value::List(items) = doc.states[0].get_mut("items").unwrap() else { panic!("items") };
        items.swap(0, 1);
        assert_eq!(doc.sync_runtime_bindings_from_scene(&runtime, None, &controls).unwrap(), 1);
        let Value::List(items) = &doc.states[0]["items"] else { panic!("items") };
        let Value::Object(first) = &items[0] else { panic!("first") };
        let Value::Object(second) = &items[1] else { panic!("second") };
        assert_eq!(first["title"], string("Beta"));
        assert_eq!(second["title"], string("edited"));
        assert_eq!(controls[0].state["__binding_frames"], frames);
    }
    #[test]
    fn flat_frame_owned_control_snapshots_survive_subsequent_model_replacement() {
        let files = BTreeMap::from([("App.ui".into(), "component App { state items: List = []; Frame { width:260; height:300; for item in state.items key item.id { TextInput { width:200; height:32; value <-> item.title; } } } }".into())]);
        let item = |id: &str, title: &str| Value::Object(props(vec![("id", string(id)), ("title", string(title))]));
        let mut doc = Document::from_ui_files(&files, "App.ui", &props(vec![("items", Value::List(vec![item("a", "Alpha"), item("b", "Beta")]))])).unwrap();
        let first = doc.render_scene_artifact().unwrap();
        assert_eq!(first.controls.len(), 2);
        doc.states[0].insert("items".into(), Value::List(vec![item("c", "Changed")]));
        let second = doc.render_scene_artifact().unwrap();
        assert_eq!(second.controls.len(), 1);
        for (control, title) in first.controls.iter().zip(["Alpha", "Beta"]) {
            let Value::Object(local) = &control.state["item"] else { panic!("item snapshot") };
            assert_eq!(local["title"], string(title));
            let Value::List(items) = &control.state["items"] else { panic!("root snapshot") };
            assert_eq!(items.len(), 2);
        }
        let runtime = crate::Runtime::from_scene_artifact(&first).unwrap();
        assert_eq!(runtime.control_count(), 2);
        assert_eq!(runtime.text_value(0), "Alpha");
        let runtime = crate::Runtime::from_scene_artifact(&second).unwrap();
        assert_eq!(runtime.control_count(), 1);
        assert_eq!(runtime.text_value(0), "Changed");
    }
    #[test]
    fn scene_snapshot_transfer_matches_borrowed_instantiation_and_restores_on_error() {
        let mut doc = document(root(vec![]));
        let state = props(vec![("caption", string("snapshot")), ("items", Value::List(vec![number(1.), number(2.)]))]);
        let mut instance = node("Control", vec![
            ("text", expr("props.__state.caption")), ("__state", Value::Object(state.clone())),
        ], vec![]);
        let original = instance.clone();
        let expected = doc.instantiate(&instance, true, &state, &[]).unwrap();
        let actual = doc.instantiate_scene(&mut instance).unwrap();
        assert_eq!(actual, expected);
        assert_eq!(instance, original);
        doc.definitions.get_mut("Control").unwrap().nodes = vec![node("Text", vec![("text", string("invalid root"))], vec![])];
        assert!(doc.instantiate_scene(&mut instance).unwrap_err().contains("Rectangle root"));
        assert_eq!(instance, original, "failed evaluation must return the owned state snapshot");
        doc.definitions.get_mut("Control").unwrap().nodes = vec![node("Rectangle", vec![], vec![])];
        doc.definitions.get_mut("Control").unwrap().defaults.insert("__state".into(), Value::Object(state));
        let mut missing_snapshot = node("Control", vec![("text", expr("state.caption"))], vec![]);
        assert!(doc.instantiate(&missing_snapshot, true, &Properties::new(), &[]).is_err());
        assert!(doc.instantiate_scene(&mut missing_snapshot).is_err());
        assert!(!missing_snapshot.props.contains_key("__state"), "definition defaults must not become the caller state");
    }
    #[test]
    fn borrowed_definition_content_is_instantiated_without_mutating_shared_templates() {
        let mut doc = document(root(vec![]));
        doc.definitions.get_mut("Control").unwrap().nodes = vec![node("Rectangle", vec![], vec![
            node("ContentPresenter", vec![("key", string("content"))], vec![]),
        ])];
        let original = doc.definitions["Control"].nodes.clone();
        let first = doc.instantiate(&node("Control", vec![], vec![
            node("Text", vec![("text", string("first"))], vec![]),
        ]), true, &Properties::new(), &[]).unwrap().0;
        let second = doc.instantiate(&node("Control", vec![], vec![
            node("Text", vec![("text", string("second"))], vec![]),
        ]), true, &Properties::new(), &[]).unwrap().0;
        assert_eq!(first.children[0].children[0].props["text"], string("first"));
        assert_eq!(second.children[0].children[0].props["text"], string("second"));
        assert_eq!(doc.definitions["Control"].nodes, original);
        let empty = doc.instantiate(&node("Control", vec![], vec![]), true, &Properties::new(), &[]).unwrap().0;
        assert!(empty.children[0].children.is_empty());
        assert_eq!(doc.definitions["Control"].nodes, original);
    }
    #[test]
    fn visual_sequence_borrowing_preserves_lexical_shadowing_and_owned_results() {
        let doc = document(root(vec![]));
        let label = || node("Text", vec![("text", expr("state.local"))], vec![]);
        let binding = |value: &str| node("Let", vec![("name", string("local")), ("value", string(value))], vec![]);
        let nodes = vec![label(), binding("later"), label(),
            node("Frame", vec![], vec![binding("inner"), label()]), label()];
        let mut state = props(vec![("local", string("parent"))]);
        let expanded = doc.expand_sequence(&nodes, &Properties::new(), &state, &[], 0).unwrap();
        assert_eq!(expanded[0].props["text"], string("parent"));
        assert_eq!(expanded[1].props["text"], string("later"));
        assert_eq!(expanded[2].children[0].props["text"], string("inner"));
        assert_eq!(expanded[3].props["text"], string("later"));
        assert_eq!(state["local"], string("parent"));
        state.insert("local".into(), string("changed"));
        assert_eq!(expanded[0].props["text"], string("parent"));
    }
    #[test]
    fn scene_scope_writes_are_local_and_returned_snapshots_remain_owned() {
        let control = || node("Control", vec![("text", expr("state.local"))], vec![]);
        let binding = |value: &str| node("Let", vec![("name", string("local")), ("value", string(value))], vec![]);
        let mut doc = document(root(vec![
            control(), control(), binding("later"), control(),
            node("Frame", vec![("width", number(180.)), ("height", number(30.))], vec![binding("inner"), control()]),
            control(),
        ]));
        doc.states = vec![Properties::new(); 9];
        doc.states[0].insert("local".into(), string("parent"));
        doc.states[2].insert("local".into(), string("node"));
        let (_, _, snapshots) = doc.render_scene().unwrap();
        let values: Vec<_> = snapshots.iter().map(|control| control.state["local"].text().unwrap()).collect();
        assert_eq!(values, ["parent", "node", "later", "inner", "later"]);
        assert_eq!(doc.states[0]["local"], string("parent"));
        doc.states[0].insert("local".into(), string("changed"));
        let (_, _, next) = doc.render_scene().unwrap();
        assert_eq!(next[0].state["local"], string("changed"));
        assert_eq!(snapshots[0].state["local"], string("parent"));
        assert_eq!(next[1].state["local"], string("node"));
        assert_eq!(next[4].state["local"], string("later"));
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

    #[test]
    fn rust_ui_compiler_resolves_inheritance_overrides_tokens_svg_and_state() {
        let mut files: BTreeMap<String, String> = BTreeMap::new();
        files.insert(
            "theme.ui".into(),
            "component Theme { tokens { brand: #3b82f6; surface: #1e293b; } enum Tone { Primary, Danger } }".into(),
        );
        files.insert(
            "icon.svg".into(),
            "<svg viewBox=\"0 0 16 16\"><circle cx=\"8\" cy=\"8\" r=\"6\" fill=\"currentColor\"/></svg>".into(),
        );
        files.insert(
            "BaseButton.component.ui".into(),
            "component BaseButton { use 'theme.ui'; prop text: String = 'Click'; prop tone: Tone = Tone.Primary; width: 120; height: 36; background: tokens.surface; match props.tone { Tone.Primary => { background: tokens.brand; } Tone.Danger => { background: #ef4444; } } Rectangle { slot: 'frame'; Row { padding: 4; gap: 6; Image { slot: 'icon'; source: 'icon.svg'; width: 16; height: 16; color: #ffffff; } Text { slot: 'label'; text: props.text; color: #ffffff; } } PointerArea { clicked -> events.clicked(); } } }".into(),
        );
        files.insert(
            "DangerButton.component.ui".into(),
            "component DangerButton : BaseButton { tone: Tone.Danger; override label { color: #ffe4e6; } }".into(),
        );
        files.insert(
            "App.ui".into(),
            "component App { state count: Number = 3; Column { width: 240; height: 120; padding: 12; gap: 10; BaseButton { key: 'primary'; text: 'Count: ${state.count}'; clicked -> actions.increment(); } DangerButton { key: 'danger'; text: 'Reset'; clicked -> actions.reset(); } } }".into(),
        );
        let doc = Document::from_ui_files(&files, "App.ui", &Properties::new()).unwrap();
        assert!(doc.images.contains_key("icon.svg"));
        let (scene_src, templates, controls) = doc.render_scene().unwrap();
        assert_eq!(controls.len(), 2);
        assert_eq!(controls[0].key, "primary");
        assert_eq!(controls[1].key, "danger");
        assert!(templates.contains("ContentText { x:"));
        assert!(templates.contains("ContentShape {"));
        let runtime = crate::Runtime::from_sources(&scene_src, &templates).unwrap();
        assert_eq!(runtime.control_count(), 2);
    }

    #[test]
    fn rust_ui_compiler_supports_apply_styles_let_match_guards_and_base_slot_wrapping() {
        let mut files: BTreeMap<String, String> = BTreeMap::new();
        files.insert(
            "Card.component.ui".into(),
            "component Card { style pill { radius: 12; } prop score: Number = 85; prop active: Bool = true; width: 180; height: 64; Rectangle { apply: pill; Column { slot: 'body'; let tier = props.score >= 80 ? 'gold' : 'silver'; match tier { 'gold' if props.active => { Text { text: 'Gold (${props.score})'; } } _ => { Text { text: 'Standard'; } } } } } }".into(),
        );
        files.insert(
            "BadgeCard.component.ui".into(),
            "component BadgeCard : Card { override body: Column { gap: 4; Text { text: 'Verified'; } }; }".into(),
        );
        files.insert(
            "App.ui".into(),
            "component App { Column { width: 220; height: 140; gap: 8; Card { key: 'card'; score: 92; active: true; } BadgeCard { key: 'badge'; score: 70; active: true; } } }".into(),
        );
        let doc = Document::from_ui_files(&files, "App.ui", &Properties::new()).unwrap();
        let (scene_src, templates, controls) = doc.render_scene().unwrap();
        assert_eq!(controls.len(), 2);
        assert_eq!(controls[0].key, "card");
        assert_eq!(controls[1].key, "badge");
        assert!(templates.contains("radius: 12;"));
        assert!(templates.contains("Gold (92)"));
        assert!(templates.contains("Verified"));
        let runtime = crate::Runtime::from_sources(&scene_src, &templates).unwrap();
        assert_eq!(runtime.control_count(), 2);
    }

    #[test]
    fn rust_ui_compiler_evaluates_and_synchronizes_two_way_bindings() {
        let mut files: BTreeMap<String, String> = BTreeMap::new();
        files.insert(
            "Toggle.component.ui".into(),
            "component Toggle { prop checked: Bool = false; prop label: String = ''; width: 140; height: 32; Rectangle { Row { padding: 4; Text { text: props.checked ? '[x] ${props.label}' : '[ ] ${props.label}'; } } PointerArea { clicked -> events.clicked(); } } }".into(),
        );
        files.insert(
            "App.ui".into(),
            "component App { state query: String = 'forma'; state agreed: Bool = false; Column { width: 260; height: 140; padding: 10; gap: 8; TextInput { key: 'search'; width: 200; height: 32; value <-> state.query; } Toggle { key: 'agree'; label: 'Terms'; checked <-> state.agreed; } Text { key: 'summary'; text: '${state.query}:${state.agreed}'; } } }".into(),
        );
        let mut doc = Document::from_ui_files(&files, "App.ui", &Properties::new()).unwrap();
        let (scene_src, templates, controls) = doc.render_scene().unwrap();
        assert_eq!(controls.len(), 3);
        let mut runtime = crate::Runtime::from_sources(&scene_src, &templates).unwrap();
        assert_eq!(runtime.text_value(0), "forma");

        // Edit the TextInput and click the Toggle
        assert!(runtime.focus_control(0));
        assert!(runtime.text_key("End", false, false));
        assert!(runtime.text_insert("-ui"));
        assert_eq!(runtime.text_value(0), "forma-ui");
        runtime.activate_control(1);

        // Synchronize bindings back to Document
        let changed = doc.sync_runtime_bindings(&runtime, Some(1)).unwrap();
        assert_eq!(changed, 2);
        let state_json = doc.state_json().unwrap();
        assert!(state_json.contains("\"query\":\"forma-ui\""));
        assert!(state_json.contains("\"agreed\":true"));

        // Re-render and verify summary Text reflects updated bound state
        let (_, next_templates, _) = doc.render_scene().unwrap();
        assert!(next_templates.contains("forma-ui:true"));
        assert!(next_templates.contains("[x] Terms"));
    }
    #[test]
    fn binding_sync_rejects_invalid_metadata_without_partial_model_writes() {
        for (cached, empty) in [(false, false), (true, false), (true, true)] {
            let files = BTreeMap::from([("App.ui".to_string(),
                "component App { state first: String = 'one'; state second: String = 'two'; Column { width:260; height:100; TextInput { width:200; height:32; value <-> state.first; } TextInput { width:200; height:32; value <-> state.second; } } }".to_string())]);
            let mut doc = Document::from_ui_files(&files, "App.ui", &Properties::new()).unwrap();
            let (source, templates, controls) = doc.render_scene().unwrap();
            let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
            runtime.set_binding_value(0, "edited", true).unwrap();
            if empty { doc.states.clear(); }
            let before = doc.states.clone();
            fn second_binding(node: &mut Node, count: &mut usize, value: Value) -> bool {
                if let Some(Value::Object(bindings)) = node.props.get_mut("__bindings") {
                    *count += 1;
                    if *count == 2 { bindings.insert("value".into(), value); return true; }
                }
                node.children.iter_mut().any(|node| second_binding(node, count, value.clone()))
            }
            assert!(second_binding(&mut doc.root, &mut 0, Value::List(vec![])));
            let result = if cached { doc.sync_runtime_bindings_from_scene(&runtime, None, &controls) }
                else { doc.sync_runtime_bindings(&runtime, None) };
            assert!(result.unwrap_err().contains("Expected scalar text"));
            assert_eq!(doc.states, before, "a later invalid binding must roll back earlier writes");
            assert!(second_binding(&mut doc.root, &mut 0, Value::Text("state.second".into())));
            let changed = if cached { doc.sync_runtime_bindings_from_scene(&runtime, None, &controls).unwrap() }
                else { doc.sync_runtime_bindings(&runtime, None).unwrap() };
            assert_eq!(changed, if empty { 2 } else { 1 });
            assert_eq!(doc.states[0]["first"], Value::Text("edited".into()));
            assert_eq!(doc.states[0]["second"], Value::Text("two".into()));
        }
    }

    #[test]
    fn recovery_binding_map_refuses_excessive_source_depth_before_writing() {
        let files = BTreeMap::from([("App.ui".to_string(),
            "component App { state value: String = 'seed'; Column { width:260; height:100; TextInput { width:200; height:32; value <-> state.value; } } }".to_string())]);
        let mut doc = Document::from_ui_files(&files, "App.ui", &Properties::new()).unwrap();
        let (source, templates, controls) = doc.render_scene().unwrap();
        let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
        runtime.set_binding_value(0, "edited", true).unwrap();
        let before = doc.states.clone();
        let mut nested = Node { kind: "Frame".into(), props: Properties::new(), children: vec![], action: None };
        for _ in 0..66 { nested = Node { kind: "Frame".into(), props: Properties::new(), children: vec![nested], action: None }; }
        doc.root.children.push(nested);
        let error = doc.sync_runtime_bindings_from_scene(&runtime, None, &controls).unwrap_err();
        assert!(error.contains("depth exceeded"));
        assert_eq!(doc.states, before);
        assert!(doc.render_scene().unwrap_err().contains("depth exceeded"));
        doc.root.children.pop();
        doc.root.children.extend((0..16_384).map(|_| Node {
            kind: "Frame".into(), props: Properties::new(), children: vec![], action: None,
        }));
        assert!(doc.sync_runtime_bindings_from_scene(&runtime, None, &controls).unwrap_err().contains("16384 nodes"));
        assert_eq!(doc.states, before);
        assert!(doc.render_scene().unwrap_err().contains("16384 nodes"));
    }

    #[test]
    fn wrapped_text_component_emits_multiline_content_and_measures_height() {
        let files = BTreeMap::from([(
            "App.ui".to_string(),
            "component App { Column { width: 200; height: 160; Text { key: 'wrapped'; width: 120; height: 'auto'; wrap: true; text: 'Hello world from Forma soft wrapped text'; } } }".to_string(),
        )]);
        let doc = Document::from_ui_files(&files, "App.ui", &Properties::new()).unwrap();
        let (scene_src, templates, controls) = doc.render_scene().unwrap();
        assert_eq!(controls.len(), 1);
        let content_text_count = templates.matches("ContentText {").count();
        assert!(
            content_text_count >= 2,
            "expected wrapped Text to emit >= 2 ContentText lines, got {content_text_count}: {templates}"
        );
        let runtime = crate::Runtime::from_sources(&scene_src, &templates).unwrap();
        assert_eq!(runtime.control_count(), 1);
    }

}
