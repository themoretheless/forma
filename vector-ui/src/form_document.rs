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
            } else if let Some(key) = path.strip_prefix("state.") {
                lookup(state, key).ok_or_else(|| format!("Missing state.{key}"))?
            } else if path == "null" {
                Value::Null
            } else if let Some(value) = enum_value(path, scope).or_else(|| lookup(state, path)) {
                value
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
                    let arguments = arguments
                        .iter()
                        .map(|value| evaluate(value, false))
                        .collect::<Result<Vec<_>, _>>()?;
                    call(name, &arguments)?
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
            _ => return Err(format!("{name} expects a list or string")),
        } as f64)),
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
            .filter(|(key, _)| !["__matches", "__forward", "__scope"].contains(&key.as_str()))
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
        let Some(value) = values.get(name) else {
            if required {
                return Err(format!("Missing required property {kind}.{name}"));
            }
            continue;
        };
        let ty = text(contract, "type", "")?;
        if ty.is_empty() {
            continue;
        }
        let optional = ty.ends_with('?');
        if optional && value == &Value::Null {
            continue;
        }
        let ty = ty.trim_end_matches('?');
        let valid = match ty {
            "String" | "Asset" => matches!(value, Value::Text(_)),
            "Color" => value.text().ok().is_some_and(|v| {
                v.starts_with('#')
                    && [4, 7, 9].contains(&v.len())
                    && v[1..].bytes().all(|b| b.is_ascii_hexdigit())
            }),
            "Bool" | "Boolean" => matches!(value, Value::Bool(_)),
            "Number" | "Float" => matches!(value,Value::Number(n) if n.is_finite()),
            "Int" => {
                matches!(value,Value::Number(n) if n.is_finite() && n.fract() == 0. && n.abs() <= 9_007_199_254_740_991.)
            }
            "Length" => {
                matches!(value,Value::Number(n) if n.is_finite() && *n>=0.)
                    || matches!(value,Value::Text(v)|Value::Expr(v) if valid_unit(v,&["px","%"]))
            }
            "Duration" => matches!(value,Value::Text(v)|Value::Expr(v) if valid_unit(v,&["ms"])),
            _ => {
                let Some(Value::Object(enums)) = enums else {
                    return Err(format!("Unknown property type {ty}"));
                };
                let Some(Value::List(variants)) = enums.get(ty) else {
                    return Err(format!("Unknown property type {ty}"));
                };
                variants.iter().any(|v| {
                    v.text()
                        .ok()
                        .map(|variant| Value::Text(format!("{ty}.{variant}")))
                        == Some(value.clone())
                })
            }
        };
        if !valid {
            return Err(format!(
                "Property {kind}.{name} expects {ty}, got {value:?}"
            ));
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
                    let points = points
                        .iter()
                        .map(|p| format!("{} {}", dx + p[0] * scale, dy + p[1] * scale))
                        .collect::<Vec<_>>()
                        .join(" ");
                    out.push_str(&format!(
                        "ContentShape {{ points:'{points}'; color:{color}; }} "
                    ));
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
        if let Some(Value::Object(enums)) = defaults.get("__enums") {
            let mut merged = match scope.remove("__enums") {
                Some(Value::Object(v)) => v,
                _ => Properties::new(),
            };
            merged.extend(enums.clone());
            scope.insert("__enums".into(), Value::Object(merged));
        }
        let mut result = Properties::new();
        let values = property_values(node, &scope, state)?;
        for (key, value) in &values {
            if key == "__matches" || key == "__forward" || key == "__scope" {
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
                    root.props.extend(p);
                    root
                })
        }
    }
    fn position_scene(
        &self,
        node: &Node,
        bounds: [f64; 4],
        instances: &mut Vec<Node>,
        root_legacy: bool,
    ) -> Result<(), String> {
        if !is_layout(&node.kind) {
            let mut node = node.clone();
            let visual = self.scene_visual(&node)?;
            let width = dimension(&visual, 0, bounds[2])?;
            let height = dimension(&visual, 1, bounds[3])?;
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
            instances.push(node);
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
        let visual = self.scene_visual(&node)?;
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
                self.position_scene(child, [cx, cy, w, h], instances, false)?;
            } else {
                self.position_scene(child, [cx, cy, cw, ch], instances, false)?;
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
        let mut instances = Vec::new();
        if advanced {
            let width = num(&root.props, "width", 360.)?;
            let height = num(&root.props, "height", 220.)?;
            self.position_scene(layout, [0., 0., width, height], &mut instances, true)?;
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
            instances = layout.children.clone();
        }
        root.kind = "Frame".into();
        let empty = Properties::new();
        let mut mapping = Vec::new();
        let mut source_controls = Vec::new();
        let mut templates = String::from("FORMA-TEMPLATES-1\n");
        for instance in &instances {
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
            let (visual, resolved) = self.instantiate(instance, true, state, &[])?;
            let mut content = String::new();
            let bounds = [
                0.,
                0.,
                dimension(
                    &Node {
                        props: resolved.clone(),
                        ..visual.clone()
                    },
                    0,
                    260.,
                )?,
                dimension(
                    &Node {
                        props: resolved.clone(),
                        ..visual.clone()
                    },
                    1,
                    70.,
                )?,
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
