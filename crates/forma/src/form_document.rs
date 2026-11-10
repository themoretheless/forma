//! Executable component definitions emitted by the form compiler. No reflection
//! or JS is used in the application: typed model accessors supply named values.
use std::collections::BTreeMap;

mod bindings;
mod compiler;
mod contracts;
mod control_flow;
mod eval;
mod layout;
mod properties;
mod render;
mod scene;
mod svg;
mod svg_path;
mod visual;
mod widget_state;

#[cfg(test)]
use bindings::{
    BindingKeyIndexes, lookup_binding_mut, lookup_binding_ref, resolve_binding_frame,
    resolve_binding_item, write_collection_item_path, write_state_path,
};
use compiler::{compile_ui_files_to_document, doc_props_to_json, json_to_doc_props};
use contracts::check_contracts;
use eval::{enum_value, eval, expression_number, lookup, pattern_matches};
#[cfg(test)]
use layout::tracks;
use layout::{
    cell, dimension, gaps, insets, normalize_layout, normalize_layout_owned, serialize_into, sizes,
    write_property_literal,
};
pub use properties::{Properties, PropertyIter};
use svg::parse_svg_shapes;
pub use widget_state::WidgetState;

// Evaluation scopes capture immutable model data, unlike the live model map.
// Their first local mutation gets a shared-root overlay instead of owning it.
fn scope_mut<'a, 'b>(scope: &'a mut std::borrow::Cow<'b, Properties>) -> &'a mut Properties {
    if matches!(scope, std::borrow::Cow::Borrowed(_)) {
        let current = scope.as_ref();
        *scope = std::borrow::Cow::Owned(
            current
                .small_scope_clone(4096)
                .unwrap_or_else(|| current.scoped_clone()),
        );
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
        if !value.is_finite() {
            return Err(serde::ser::Error::custom("Nonfinite scene value"));
        }
        serializer.serialize_f64(*value)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
        let value = <f64 as serde::Deserialize>::deserialize(deserializer)?;
        if !value.is_finite() {
            return Err(serde::de::Error::custom("Nonfinite scene value"));
        }
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
fn deserialize_content_end<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<[f64; 2]>, D::Error> {
    serde::Deserialize::deserialize(deserializer)
}
impl SceneArtifact {
    pub fn to_json(&self) -> Result<String, String> {
        if self
            .logical_content_end
            .is_some_and(|end| end.iter().any(|v| !v.is_finite() || *v < 0.))
        {
            return Err("Invalid logical content extent".into());
        }
        #[derive(serde::Serialize)]
        struct Wire<'a> {
            protocol: &'a str,
            source: &'a str,
            templates: &'a str,
            controls: &'a [SceneControl],
            logical_content_end: Option<[f64; 2]>,
        }
        serde_json::to_string(&Wire {
            protocol: SCENE_ARTIFACT_PROTOCOL,
            source: &self.source,
            templates: &self.templates,
            controls: &self.controls,
            logical_content_end: self.logical_content_end,
        })
        .map_err(|e| e.to_string())
    }
    pub fn from_json(json: &str) -> Result<Self, String> {
        let wire: SceneArtifactWire = serde_json::from_str(json).map_err(|e| e.to_string())?;
        if wire.protocol != SCENE_ARTIFACT_PROTOCOL {
            return Err("Unsupported scene artifact protocol".into());
        }
        if wire
            .logical_content_end
            .is_some_and(|end| end.iter().any(|v| !v.is_finite() || *v < 0.))
        {
            return Err("Invalid logical content extent".into());
        }
        Ok(Self {
            source: wire.source,
            templates: wire.templates,
            controls: wire.controls,
            logical_content_end: wire.logical_content_end,
        })
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
            Self::Text(s) => write_str_literal(s, out),
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
                return Err("Unevaluated expression".into());
            }
            Self::Object(_) | Self::Null => return Err("Expected serializable visual value".into()),
        }
        Ok(())
    }
}
fn write_str_literal(s: &str, out: &mut String) {
    out.push('\'');
    if !s
        .bytes()
        .any(|b| matches!(b, b'\\' | b'\'' | b'\n' | b'\r' | b'\t'))
    {
        out.push_str(s);
    } else {
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
    }
    out.push('\'');
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
            .filter(|(key, _)| {
                !["__matches", "__forward", "__scope", "__styles"].contains(&key.as_str())
            })
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
// Retain evaluated Let values separately from mutable model fields. A Let
// declaration also hides an older For local with the same name.
fn insert_scene_lexical(
    state: &mut Properties,
    name: String,
    value: Value,
    expression: &Value,
    scope: &Properties,
) {
    let parents = state
        .get("__binding_active")
        .cloned()
        .unwrap_or_else(|| Value::Object(Properties::new()));
    let steps = state.get_or_insert_with("__binding_let_steps".into(), || Value::List(Vec::new()));
    if let Value::List(steps) = steps {
        steps.push(Value::Object(Properties::from([
            ("name".into(), Value::Text(name.clone())),
            ("expression".into(), expression.clone()),
            ("scope".into(), Value::Object(scope.clone())),
            ("parents".into(), parents),
        ])));
    }
    let lexicals = state.get_or_insert_with("__binding_lexicals".into(), || {
        Value::Object(Properties::new())
    });
    if let Value::Object(lexicals) = lexicals {
        lexicals.insert(name.clone(), value.clone());
    }
    if let Some(Value::Object(active)) = state.get_mut("__binding_active") {
        active.remove(&name);
    }
    for prefix in [
        "__collection",
        "__binding_key",
        "__binding_key_expr",
        "__binding_key_scope",
    ] {
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
    /// Parses an SVG document (`svg`, `rect`, `circle`, `ellipse`, `line`, `polyline`, `polygon`, `path`)
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
}

fn count_scene_subtree(node: &Node, index: &mut usize, depth: usize) -> Result<(), String> {
    if depth > 64 {
        return Err("Scene composition depth exceeded".into());
    }
    if *index >= 16_384 {
        return Err("Scene exceeds 16384 nodes".into());
    }
    *index += 1;
    for child in &node.children {
        count_scene_subtree(child, index, depth + 1)?;
    }
    Ok(())
}

fn collect_preorder_nodes<'a>(
    node: &'a Node,
    out: &mut Vec<&'a Node>,
    depth: usize,
) -> Result<(), String> {
    if depth > 64 {
        return Err("Scene composition depth exceeded".into());
    }
    if out.len() >= 16_384 {
        return Err("Scene exceeds 16384 nodes".into());
    }
    out.push(node);
    for child in &node.children {
        collect_preorder_nodes(child, out, depth + 1)?;
    }
    Ok(())
}

// Check each structural level before cloning its children. A clone-then-check
// approach can overflow the stack on malformed externally supplied trees.
fn annotated_scene_source(node: &Node, index: &mut usize, depth: usize) -> Result<Node, String> {
    if depth > 64 {
        return Err("Scene composition depth exceeded".into());
    }
    if *index >= 16_384 {
        return Err("Scene exceeds 16384 nodes".into());
    }
    let mut props = node.props.clone();
    props
        .entry("__node".into())
        .or_insert(number(*index as f64));
    *index += 1;
    let mut children = Vec::with_capacity(node.children.len().min(16_384 - *index));
    for child in &node.children {
        children.push(annotated_scene_source(child, index, depth + 1)?);
    }
    Ok(Node {
        kind: node.kind.clone(),
        props,
        children,
        action: node.action.clone(),
    })
}

#[cfg(test)]
mod tests;
