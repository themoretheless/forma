//! Executable component definitions emitted by the form compiler. No reflection
//! or JS is used in the application: typed model accessors supply named values.
use std::collections::BTreeMap;

mod bindings;
mod compiler;
mod contracts;
mod eval;
mod layout;
mod properties;
mod svg;

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
/// `fontFamily` (or `font.family`) of a passive Text: `'sans'` by default, or `'mono'`.
fn font_family(p: &Properties) -> Result<crate::text::FontFamily, String> {
    let name = text(p, "fontFamily", &text(p, "font.family", "sans")?)?;
    crate::text::FontFamily::from_name(&name)
        .ok_or_else(|| format!("Text.fontFamily expects 'sans' or 'mono', got '{name}'"))
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
    fn expand_sequence<'a>(
        &self,
        nodes: impl IntoIterator<Item = &'a Node>,
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
                    if condition {
                        let body_children: Vec<&Node> =
                            node.children.iter().filter(|n| n.kind != "Else").collect();
                        result.extend(self.expand_sequence(
                            body_children.iter().copied(),
                            props,
                            &current_state,
                            parents,
                            depth + 1,
                        )?);
                    } else if let Some(else_node) = node.children.iter().find(|n| n.kind == "Else")
                    {
                        result.extend(self.expand_sequence(
                            &else_node.children,
                            props,
                            &current_state,
                            parents,
                            depth + 1,
                        )?);
                    }
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
                    let idx_key = format!("__index.{item}");
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
                    let body_children: Vec<&Node> =
                        node.children.iter().filter(|n| n.kind != "Empty").collect();
                    for (index, value) in items.into_iter().enumerate() {
                        let mut state = current_state.as_ref().scoped_clone();
                        state.insert(item.clone(), value);
                        state.insert(idx_key.clone(), number(index as f64));
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
                        result.extend(self.expand_sequence(
                            body_children.iter().copied(),
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
        let supplied_state = if snapshot.is_none() {
            Some(&empty_state)
        } else {
            None
        };
        let result = self.instantiate_scope(instance, true, supplied_state, &[], &mut snapshot);
        if let Some(snapshot) = snapshot {
            instance.props.insert("__state".into(), snapshot);
        }
        result
    }
    fn instantiate_scope(
        &self,
        instance: &Node,
        top: bool,
        supplied_state: Option<&Properties>,
        parents: &[String],
        owned_snapshot: &mut Option<Value>,
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
        if let Some(snapshot) = owned_snapshot.take() {
            props.insert("__state".into(), snapshot);
        }
        let empty_state = Properties::new();
        let result = (|| {
            let state = supplied_state.unwrap_or_else(|| match props.get("__state") {
                Some(Value::Object(state)) => state,
                _ => &empty_state,
            });
            let mut selected = select_groups(definition.defaults.get("__matches"), &props, state)?;
            // Explicit scene metadata wins over group defaults without cloning its payload.
            if transferred_snapshot {
                selected.remove("__state");
            }
            // Explicit instance values win over selected groups. Only keys actually
            // overwritten by a group need to be restored; transport snapshots such
            // as __state must not be cloned a second time on every instantiation.
            let overrides: Properties = selected
                .keys()
                .filter_map(|key| {
                    instance
                        .props
                        .get(key)
                        .map(|value| (key.clone(), value.clone()))
                })
                .collect();
            props.extend(selected);
            props.extend(overrides);
            if let Some(value) = instance.props.get("font.size") {
                props.insert("fontSize".into(), value.clone());
            }
            let mut parents = parents.to_vec();
            parents.push(instance.kind.clone());
            let state = supplied_state.unwrap_or_else(|| match props.get("__state") {
                Some(Value::Object(state)) => state,
                _ => &empty_state,
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
                        if contract_values.contains_key(key) {
                            contract_values.to_mut().remove(key);
                        }
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
        if transferred_snapshot {
            *owned_snapshot = props.remove("__state");
        }
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
                format_args!(
                    "ContentClip {{ x:{x}; y:{y}; width:{w}; height:{h}; radius:{radius}; }} "
                ),
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
                let mut col_offsets = Vec::with_capacity(cols.len());
                let mut col_acc = 0.0;
                for (i, &cw) in cols.iter().enumerate() {
                    col_offsets.push(col_acc + i as f64 * gap[1]);
                    col_acc += cw;
                }
                let mut row_offsets = Vec::with_capacity(rows.len());
                let mut row_acc = 0.0;
                for (i, &rh) in rows.iter().enumerate() {
                    row_offsets.push(row_acc + i as f64 * gap[0]);
                    row_acc += rh;
                }
                for c in &n.children {
                    let ci = cell(c, 0, cols.len())?;
                    let ri = cell(c, 1, rows.len())?;
                    self.flatten(
                        c,
                        [
                            x + ci.map_or(0., |i| col_offsets[i]),
                            y + ri.map_or(0., |i| row_offsets[i]),
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
                write_str_literal(&text(&n.props, "value", "")?, out);
                out.push_str("; label:");
                write_str_literal(&text(&n.props, "label", "")?, out);
                out.push_str("; placeholder:");
                write_str_literal(&text(&n.props, "placeholder", "")?, out);
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
                let family = font_family(&n.props)?;
                // The default family is left implicit, so existing scenes stay byte-identical.
                let family_field = match family {
                    crate::text::FontFamily::Sans => String::new(),
                    other => format!(" fontFamily:{};", other.as_str()),
                };
                if wrap && w > 0. {
                    let wrapped =
                        crate::text::wrap_text_ranges_in(family, &value, fs as f32, w as f32);
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
                        write_str_literal(line, out);
                        let _ = std::fmt::Write::write_fmt(
                            out,
                            format_args!("; color:{color}; fontSize:{fs};{family_field} }} "),
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
                                crate::text::measure_line_in(family, line, fs as f32)[0] as f64,
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
                        write_str_literal(line, out);
                        let _ = std::fmt::Write::write_fmt(
                            out,
                            format_args!("; color:{color}; fontSize:{fs};{family_field} }} "),
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
        let default_value = |key: &str| {
            definition
                .and_then(|d| d.defaults.get(key))
                .or_else(|| self.fallback.get(key))
        };
        let mut scope = std::borrow::Cow::Borrowed(scope);
        for meta_key in ["__enums", "__tokens", "__i18n", "__lets", "__styles"] {
            if let Some(Value::Object(meta_obj)) = default_value(meta_key) {
                let scope = scope_mut(&mut scope);
                let mut merged = match scope.remove(meta_key) {
                    Some(Value::Object(v)) => v
                        .small_scope_clone(4096)
                        .unwrap_or_else(|| v.scoped_clone()),
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
        &self,
        children: impl IntoIterator<Item = &'a Node>,
        inherited: &Properties,
        prefix: &str,
        scope: &Properties,
        depth: usize,
        budget: &mut usize,
        node_index: &mut usize,
    ) -> Result<Vec<Node>, String> {
        let mut state = std::borrow::Cow::Borrowed(inherited);
        let mut out = Vec::new();
        for child in children {
            if child.kind == "Let" {
                let mut child_node_index = *node_index;
                count_scene_subtree(child, &mut child_node_index, depth)?;
                if *budget == 0 {
                    return Err("Scene exceeds 16384 nodes".into());
                }
                *budget -= 1;
                let mut seq_index = *node_index + 1;
                *node_index = child_node_index;
                let name = text(&child.props, "name", "")?;
                if !name.is_empty() {
                    if let Some(expression) = child.props.get("value") {
                        let value = expression.evaluate(scope, &state)?;
                        insert_scene_lexical(scope_mut(&mut state), name, value, expression, scope);
                    }
                }
                out.extend(self.scene_expand_sequence(
                    &child.children,
                    &state,
                    prefix,
                    scope,
                    depth + 1,
                    budget,
                    &mut seq_index,
                )?);
            } else {
                out.extend(
                    self.scene_expand(child, &state, prefix, scope, depth, budget, node_index)?,
                );
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
        node_index: &mut usize,
    ) -> Result<Vec<Node>, String> {
        let start_node_index = *node_index;
        let mut subtree_end = start_node_index;
        count_scene_subtree(node, &mut subtree_end, depth)?;
        *node_index = subtree_end;
        if *budget == 0 {
            return Err("Scene exceeds 16384 nodes".into());
        }
        *budget -= 1;
        let id = match node.props.get("__node") {
            Some(v) => v.number()? as usize,
            None => start_node_index,
        };
        let mut state = std::borrow::Cow::Borrowed(inherited);
        if let Some(local) = self.states.get(id) {
            if !local.is_empty() {
                scope_mut(&mut state).extend(local.clone());
            }
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
                        if contract_values.contains_key(key) {
                            contract_values.to_mut().remove(key);
                        }
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
                let mut child_idx = start_node_index + 1;
                if condition {
                    let mut then_nodes = Vec::with_capacity(node.children.len());
                    for child in &node.children {
                        if child.kind == "Else" {
                            count_scene_subtree(child, &mut child_idx, depth + 1)?;
                        } else {
                            then_nodes.push(child);
                        }
                    }
                    self.scene_expand_sequence(
                        then_nodes,
                        &state,
                        prefix,
                        &scope,
                        depth + 1,
                        budget,
                        &mut child_idx,
                    )
                } else {
                    for child in &node.children {
                        if child.kind == "Else" {
                            let mut else_idx = child_idx + 1;
                            return self.scene_expand_sequence(
                                &child.children,
                                &state,
                                prefix,
                                &scope,
                                depth + 1,
                                budget,
                                &mut else_idx,
                            );
                        }
                        count_scene_subtree(child, &mut child_idx, depth + 1)?;
                    }
                    Ok(Vec::new())
                }
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
                let collection_expr = node.props.get("collection").and_then(|v| v.text().ok());
                let mut out = Vec::new();
                let mut keys = std::collections::BTreeSet::new();
                if items.is_empty() {
                    let mut child_idx = start_node_index + 1;
                    for child in &node.children {
                        if child.kind == "Empty" {
                            let mut empty_idx = child_idx + 1;
                            for empty_child in &child.children {
                                out.extend(self.scene_expand(
                                    empty_child,
                                    &state,
                                    prefix,
                                    &scope,
                                    depth + 1,
                                    budget,
                                    &mut empty_idx,
                                )?);
                            }
                            break;
                        }
                        count_scene_subtree(child, &mut child_idx, depth + 1)?;
                    }
                }
                // Small loops with little owned data keep the cheaper flat
                // map path. Large loops/payloads retain shared-root overlays.
                let mut local = std::borrow::Cow::Owned(
                    if items.len() <= 32 {
                        state.small_scope_clone(4096)
                    } else {
                        None
                    }
                    .unwrap_or_else(|| state.as_ref().scoped_clone()),
                );
                // These captured outer locals are identical for every item.
                // Materialize them only after the first valid key, then share
                // the immutable map across retained binding frames.
                let mut captured_locals: Option<Properties> = None;
                let idx_key = format!("__index.{item}");
                let col_key = format!("__collection.{item}");
                let binding_metadata = [
                    format!("__binding_key.{item}"),
                    format!("__binding_key_expr.{item}"),
                    format!("__binding_key_scope.{item}"),
                ];
                let body_children: Vec<&Node> =
                    node.children.iter().filter(|n| n.kind != "Empty").collect();
                for (index, value) in items.into_iter().enumerate() {
                    let local = scope_mut(&mut local);
                    // Key evaluation sees the inherited metadata, never the
                    // previous iteration's key (also when nested For shadows item).
                    for name in &binding_metadata {
                        match state.get(name) {
                            Some(value) => {
                                local.set(name.clone(), value.clone());
                            }
                            None => {
                                local.unset(name);
                            }
                        }
                    }
                    local.set(item.clone(), value);
                    if let Some(Value::Object(lexicals)) = local.get_mut("__binding_lexicals") {
                        if lexicals.contains_key(&item) {
                            let mut masked = lexicals.scoped_clone();
                            masked.unset(&item);
                            *lexicals = masked;
                        }
                    }
                    local.set(idx_key.clone(), number(index as f64));
                    if let Some(col_expr) = &collection_expr {
                        local.set(col_key.clone(), Value::Text(col_expr.clone()));
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
                    local.set(binding_metadata[0].clone(), Value::Text(key.clone()));
                    local.set(binding_metadata[1].clone(), node.props["key"].clone());
                    local.set(
                        binding_metadata[2].clone(),
                        Value::Object(scope.as_ref().clone()),
                    );
                    let mut frames = match state.get("__binding_frames") {
                        Some(Value::List(frames)) => frames.clone(),
                        _ => Vec::new(),
                    };
                    let mut active = match state.get("__binding_active") {
                        Some(Value::Object(active)) => active.clone(),
                        _ => Properties::new(),
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
                    if let Some(value) = local.get(&col_key) {
                        frame.insert(col_key.clone(), value.clone());
                    }
                    for name in &binding_metadata {
                        if let Some(value) = local.get(name) {
                            frame.insert(name.clone(), value.clone());
                        }
                    }
                    active.insert(item.clone(), number(frames.len() as f64));
                    frames.push(Value::Object(frame));
                    local.set("__binding_frames".into(), Value::List(frames));
                    local.set("__binding_active".into(), Value::Object(active));
                    let prefix = format!("{prefix}@{id}:{key}/");
                    let mut iter_idx = start_node_index + 1;
                    out.extend(self.scene_expand_sequence(
                        body_children.iter().copied(),
                        local,
                        &prefix,
                        &scope,
                        depth + 1,
                        budget,
                        &mut iter_idx,
                    )?);
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
                        let mut child_idx = start_node_index + 1;
                        out.extend(self.scene_expand_sequence(
                            &node.children,
                            &next_state,
                            prefix,
                            &scope,
                            depth + 1,
                            budget,
                            &mut child_idx,
                        )?);
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
                let mut child_idx = start_node_index + 1;
                for branch in &node.children {
                    let branch_start = child_idx;
                    count_scene_subtree(branch, &mut child_idx, depth + 1)?;
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
                            let mut branch_body_idx = branch_start + 1;
                            out.extend(self.scene_expand_sequence(
                                &branch.children,
                                &state,
                                prefix,
                                &scope,
                                depth + 1,
                                budget,
                                &mut branch_body_idx,
                            )?);
                            break;
                        }
                    }
                }
                Ok(out)
            }
            _ => {
                let mut props = self.scene_properties(node, &scope, &state)?;
                props.entry("__node".into()).or_insert(number(id as f64));
                let mut output = Node {
                    kind: node.kind.clone(),
                    props,
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
                    let mut child_idx = start_node_index + 1;
                    let mut annotated_children = Vec::with_capacity(node.children.len());
                    for child in &node.children {
                        annotated_children.push(annotated_scene_source(
                            child,
                            &mut child_idx,
                            depth + 1,
                        )?);
                    }
                    output.children = annotated_children;
                } else {
                    let mut child_idx = start_node_index + 1;
                    output.children = self.scene_expand_sequence(
                        &node.children,
                        &state,
                        prefix,
                        &scope,
                        depth + 1,
                        budget,
                        &mut child_idx,
                    )?;
                }
                // Children and resolved properties no longer borrow this scope.
                // Move a local owned state into its public control snapshot;
                // inherited shared state is cloned only when ownership is needed.
                // Layout-only nodes do not consume snapshots.
                if !is_layout(&node.kind) {
                    output
                        .props
                        .insert("__state".into(), Value::Object(state.into_owned()));
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
                children: node
                    .children
                    .iter_mut()
                    .map(|n| self.scene_visual(n))
                    .collect::<Result<_, _>>()?,
                action: node.action.clone(),
            };
            normalize_layout_owned(visual)
        } else {
            self.instantiate_scene(node).map(|(mut root, p)| {
                let orig = root.props.clone();
                root.props.extend(p);
                root.props
                    .insert("__rect_props".into(), Value::Object(orig));
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
            if scroll_content {
                offset.max(0.)
            } else {
                offset
            }
        };
        if is_layout(&node.kind) {
            logical_end[0] = logical_end[0].max(bounds[0] + bounds[2]);
            logical_end[1] = logical_end[1].max(bounds[1] + bounds[3]);
        }
        if !is_layout(&node.kind) {
            let visual = match precomputed_visual {
                Some(v) => v,
                None => self.scene_visual(&mut node)?,
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
            _ => self.scene_visual(&mut node)?,
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
        let mut col_offsets = Vec::with_capacity(cols.len());
        let mut col_acc = 0.0;
        for (i, &cw) in cols.iter().enumerate() {
            col_offsets.push(col_acc + i as f64 * gap[1]);
            col_acc += cw;
        }
        let mut row_offsets = Vec::with_capacity(rows.len());
        let mut row_acc = 0.0;
        for (i, &rh) in rows.iter().enumerate() {
            row_offsets.push(row_acc + i as f64 * gap[0]);
            row_acc += rh;
        }
        for (child, child_visual) in node.children.into_iter().zip(visual.children) {
            let ci = cell(&child, 0, cols.len())?;
            let ri = cell(&child, 1, rows.len())?;
            let cw = ci.map_or(width, |i| cols[i]);
            let ch = ri.map_or(height, |i| rows[i]);
            let mut cx = x + ci.map_or(0., |i| col_offsets[i]);
            let mut cy = y + ri.map_or(0., |i| row_offsets[i]);
            if is_layout(&child.kind) {
                let w = dimension(&child_visual, 0, cw)?;
                let h = dimension(&child_visual, 1, ch)?;
                cx += align(cw, w);
                cy += align(ch, h);
                self.position_scene(
                    child,
                    Some(child_visual),
                    [cx, cy, w, h],
                    instances,
                    false,
                    logical_end,
                    scroll_content,
                )?;
            } else {
                self.position_scene(
                    child,
                    Some(child_visual),
                    [cx, cy, cw, ch],
                    instances,
                    false,
                    logical_end,
                    scroll_content,
                )?;
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
        let mut node_index = 0usize;
        let mut expanded = self.scene_expand(
            &self.root,
            &Properties::new(),
            "",
            &Properties::new(),
            0,
            &mut 16_384,
            &mut node_index,
        )?;
        if expanded.len() != 1 || !is_layout(&expanded[0].kind) {
            return Err("A scene requires one layout root".into());
        }
        fn collect(mut node: Node, controls: &mut Vec<SceneControl>) -> Result<(), String> {
            if is_layout(&node.kind) {
                for child in node.children {
                    collect(child, controls)?;
                }
            } else {
                let id = num(&node.props, "__node", 0.)? as usize;
                let key = text(&node.props, "key", "")?;
                let state = match node.props.remove("__state") {
                    Some(Value::Object(state)) => state,
                    _ => Properties::new(),
                };
                let event_args = match node.props.remove("__event_args") {
                    Some(Value::Object(events)) => events
                        .into_iter()
                        .map(|(name, value)| {
                            let Value::List(values) = value else {
                                return Err("Event arguments must be a list".to_string());
                            };
                            Ok((name, values))
                        })
                        .collect::<Result<_, String>>()?,
                    _ => BTreeMap::new(),
                };
                controls.push(SceneControl {
                    node: id,
                    key,
                    state: compact_scene_control_state(state),
                    event_args,
                });
            }
            Ok(())
        }
        let mut controls = Vec::new();
        collect(expanded.remove(0), &mut controls)?;
        Ok(controls)
    }
    pub fn render_scene(&self) -> Result<(String, String, Vec<SceneControl>), String> {
        self.render_scene_with_extents()
            .map(|(source, templates, controls, _)| (source, templates, controls))
    }
    /// Render an owned artifact for `Runtime::from_scene_artifact`, retaining empty layout extents.
    pub fn render_scene_artifact(&self) -> Result<SceneArtifact, String> {
        let (source, templates, controls, logical_content_end) =
            self.render_scene_with_extents()?;
        Ok(SceneArtifact {
            source,
            templates,
            controls,
            logical_content_end,
        })
    }
    /// Runtime metadata retains logical layout bounds that serialized control leaves cannot express.
    pub(crate) fn render_scene_with_extents(
        &self,
    ) -> Result<(String, String, Vec<SceneControl>, Option<[f64; 2]>), String> {
        let mut node_index = 0usize;
        let mut expanded = self.scene_expand(
            &self.root,
            &Properties::new(),
            "",
            &Properties::new(),
            0,
            &mut 16_384,
            &mut node_index,
        )?;
        if expanded.len() != 1 {
            return Err("A scene requires one layout root".into());
        }
        let mut root = expanded.remove(0);
        if !is_layout(&root.kind) {
            return Err("A scene requires a layout root".into());
        }
        let measuring = if root.kind == "Frame"
            && !root.props.contains_key("columns")
            && !root.props.contains_key("rows")
        {
            let kind = std::mem::replace(&mut root.kind, "Column".into());
            let measured = self.scene_visual(&mut root);
            root.kind = kind;
            measured?
        } else {
            self.scene_visual(&mut root)?
        };
        let root_width = dimension(&measuring, 0, 360.)?;
        let root_height = dimension(&measuring, 1, 220.)?;
        root.props.insert("width".into(), number(root_width));
        root.props.insert("height".into(), number(root_height));
        let scroll = root.children.len() == 1 && root.children[0].kind == "Scroll";
        let scroll_viewport = if scroll {
            let layout = &root.children[0];
            for key in layout.props.keys() {
                if !key.starts_with("__")
                    && ![
                        "key",
                        "width",
                        "height",
                        "minWidth",
                        "maxWidth",
                        "minHeight",
                        "maxHeight",
                        "padding",
                        "gap",
                        "columns",
                        "rows",
                    ]
                    .contains(&key.as_str())
                {
                    return Err(format!("Unsupported root Scroll property: {key}"));
                }
            }
            if layout.action.is_some() {
                return Err("Root Scroll actions are unsupported".into());
            }
            let padding = insets(root.props.get("padding"))?;
            let available = [
                root_width - padding[1] - padding[3],
                root_height - padding[0] - padding[2],
            ];
            let visual = measuring.children.first().ok_or("Missing Scroll visual")?;
            let width = dimension(visual, 0, available[0])?;
            let height = dimension(visual, 1, available[1])?;
            if width < 0. || height < 0. || width > available[0] || height > available[1] {
                return Err("Root Scroll viewport must fit inside the scene content area".into());
            }
            Some([
                padding[3] + (available[0] - width) / 2.,
                padding[0] + (available[1] - height) / 2.,
                width,
                height,
            ])
        } else {
            None
        };
        let layout = if scroll { &root.children[0] } else { &root };
        let advanced = scroll
            || layout.props.contains_key("columns")
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
            self.position_scene(
                positioned,
                precomputed,
                scroll_viewport.unwrap_or([0., 0., width, height]),
                &mut instances,
                true,
                &mut logical_end,
                scroll,
            )?;
            for key in ["columns", "rows", "padding", "gap"] {
                root.props.remove(key);
            }
            root.props.insert("padding".into(), number(0.));
            root.props.insert("gap".into(), number(0.));
            if scroll {
                let [x, y, width, height] = scroll_viewport.unwrap();
                root.props.insert(
                    "padding".into(),
                    Value::List(vec![
                        number(y),
                        number(root_width - x - width),
                        number(root_height - y - height),
                        number(x),
                    ]),
                );
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
                instances = children
                    .into_iter()
                    .zip(vis.children.into_iter().map(Some))
                    .collect();
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
            const FLATTENABLE_KINDS: [&str; 9] = [
                "Frame",
                "Row",
                "Column",
                "Grid",
                "Stack",
                "Text",
                "TextInput",
                "Image",
                "Rectangle",
            ];
            for child in &visual.children {
                if FLATTENABLE_KINDS.contains(&child.kind.as_str()) {
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
                let _ = std::fmt::Write::write_fmt(
                    &mut content,
                    format_args!("RangeInput {{ value:{v}; "),
                );
                for (name, value_str) in [("Minimum", "0%"), ("Maximum", "100%")] {
                    let mut endpoint = instance.clone();
                    endpoint
                        .props
                        .insert("value".into(), Value::Expr(value_str.into()));
                    let (visual, _) = self.instantiate(&endpoint, true, state, &[])?;
                    content.push_str(name);
                    content.push_str(" { ");
                    for c in &visual.children {
                        if FLATTENABLE_KINDS.contains(&c.kind.as_str()) {
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
            let _ = std::fmt::Write::write_fmt(
                &mut templates,
                format_args!("{}\n{template}", template.len()),
            );
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
                if n.kind == "Scroll"
                    || k.starts_with("__")
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
        Ok((
            scene_source,
            templates,
            mapping,
            scroll.then_some(logical_end),
        ))
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
        assert!(
            binary("/", number(1.), number(0.))
                .evaluate(&empty, &state)
                .is_err()
        );
        assert!(
            binary("+", string("a"), number(1.))
                .evaluate(&empty, &state)
                .is_err()
        );
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
        assert!(
            binary(
                "??",
                binary("+", expr("state.missing"), number(1.)),
                number(42.)
            )
            .evaluate(&empty, &state)
            .is_err()
        );
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
        assert!(
            doc.instantiate(
                &node("Control", vec![], vec![]),
                true,
                &Properties::new(),
                &[]
            )
            .unwrap_err()
            .contains("required")
        );
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
        assert!(
            doc.instantiate(&bad, true, &Properties::new(), &[])
                .unwrap_err()
                .contains("Unknown enum variant")
        );
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
        let mut doc =
            Document::from_ui_files(&files, "App.ui", &props(vec![("payload", payload)])).unwrap();
        let (_, _, mut controls) = doc.render_scene().unwrap();
        assert_eq!(controls.len(), 2);
        assert!(
            controls[0].state.shares_storage(&controls[1].state),
            "siblings must share the expanded root map"
        );
        let before = serde_json::to_string(&controls[1]).unwrap();
        controls[0]
            .state
            .insert("query".into(), string("snapshot-edit"));
        doc.states[0].insert("query".into(), string("model-edit"));
        assert_eq!(serde_json::to_string(&controls[1]).unwrap(), before);
        assert_eq!(controls[0].state["query"], string("snapshot-edit"));
        assert_eq!(doc.states[0]["query"], string("model-edit"));
    }

    #[test]
    fn shared_property_snapshots_detach_on_every_mutable_access() {
        let mut original = props(vec![
            (
                "object",
                Value::Object(props(vec![("caption", string("before"))])),
            ),
            ("list", Value::List(vec![string("first")])),
        ]);
        let snapshot = original.clone();
        assert!(original.shares_storage(&snapshot));
        let expected_json = serde_json::to_string(&snapshot).unwrap();
        let Some(Value::Object(object)) = original.get_mut("object") else {
            panic!("object")
        };
        object.insert("caption".into(), string("after"));
        let Some(Value::List(list)) = original.get_mut("list") else {
            panic!("list")
        };
        list.push(string("second"));
        assert!(!original.shares_storage(&snapshot));
        assert_eq!(serde_json::to_string(&snapshot).unwrap(), expected_json);
        assert_ne!(original, snapshot);
        let restored: Properties = serde_json::from_str(&expected_json).unwrap();
        assert_eq!(restored, snapshot);
        let mut detached = snapshot.clone();
        for (_, value) in &mut detached {
            *value = Value::Null;
        }
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
        definition
            .defaults
            .insert("seed".into(), string("definition"));
        definition
            .defaults
            .insert("text".into(), expr("props.seed"));
        let instance = node("Control", vec![("text", expr("state.absent"))], vec![]);
        let resolved = doc
            .scene_properties(&instance, &Properties::new(), &Properties::new())
            .unwrap();
        assert_eq!(resolved["text"], string("definition"));
        let plain = node("Text", vec![("text", expr("state.absent"))], vec![]);
        assert_eq!(
            doc.scene_properties(&plain, &Properties::new(), &Properties::new())
                .unwrap()["text"],
            string("fallback")
        );
        let state = props(vec![("absent", string("live"))]);
        assert_eq!(
            doc.scene_properties(&instance, &Properties::new(), &state)
                .unwrap()["text"],
            string("live")
        );
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
        let (_, explicit) = doc
            .instantiate(
                &node(
                    "Control",
                    vec![("flag", Value::Bool(false)), ("text", string("explicit"))],
                    vec![],
                ),
                true,
                &Properties::new(),
                &[],
            )
            .unwrap();
        assert_eq!(explicit["flag"], Value::Bool(false));
        assert_eq!(explicit["text"], string("explicit"));
        let (_, metadata) = doc
            .instantiate(
                &node(
                    "Control",
                    vec![
                        ("flag", Value::Bool(false)),
                        ("text", expr("props.__state.caption")),
                        (
                            "__state",
                            Value::Object(props(vec![("caption", string("snapshot"))])),
                        ),
                    ],
                    vec![],
                ),
                true,
                &Properties::new(),
                &[],
            )
            .unwrap();
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
        assert!(
            doc.render_scene()
                .unwrap_err()
                .contains("Duplicate collection key")
        );
    }
    #[test]
    fn collection_binding_activation_follows_key_after_reorder_or_removal() {
        for (remove, cached) in [(false, false), (true, false), (false, true), (true, true)] {
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
                            "__bindings",
                            Value::Object(props(vec![("checked", string("item.done"))])),
                        ),
                    ],
                    vec![],
                )],
            )]));
            let item = |id: &str| {
                Value::Object(props(vec![
                    ("id", string(id)),
                    ("title", string(id)),
                    ("done", Value::Bool(false)),
                ]))
            };
            doc.states = vec![props(vec![(
                "items",
                Value::List(vec![item("a"), item("b")]),
            )])];
            let (source, templates, controls) = doc.render_scene().unwrap();
            let runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
            let items = if remove {
                vec![item("b")]
            } else {
                vec![item("b"), item("a")]
            };
            doc.states[0].insert("items".into(), Value::List(items));
            let changed = if cached {
                doc.sync_runtime_bindings_from_scene(&runtime, Some(0), &controls)
                    .unwrap()
            } else {
                doc.sync_runtime_bindings(&runtime, Some(0)).unwrap()
            };
            assert_eq!(changed, if remove { 0 } else { 1 });
            let repeated = doc
                .sync_runtime_binding_events(&runtime, &[0, 0], &controls)
                .unwrap();
            assert_eq!(repeated, if remove { 0 } else { 2 });
            let Value::List(items) = &doc.states[0]["items"] else {
                panic!("items");
            };
            let Value::Object(b) = &items[0] else {
                panic!("b");
            };
            assert_eq!(
                b["done"],
                Value::Bool(false),
                "the old first index must not target b"
            );
            if !remove {
                let Value::Object(a) = &items[1] else {
                    panic!("a");
                };
                assert_eq!(a["done"], Value::Bool(true));
            }
        }
    }
    #[test]
    fn collection_editor_values_follow_keys_and_do_not_overwrite_new_items() {
        for (mode, cached) in (0..4).flat_map(|mode| [(mode, false), (mode, true)]) {
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
                        ("value", expr("item.title")),
                        (
                            "__bindings",
                            Value::Object(props(vec![("value", string("item.title"))])),
                        ),
                    ],
                    vec![],
                )],
            )]));
            let definition = doc.definitions.get_mut("Control").unwrap();
            definition.defaults.insert("value".into(), string(""));
            definition.nodes = vec![node(
                "Rectangle",
                vec![],
                vec![node(
                    "TextInput",
                    vec![("value", expr("props.value"))],
                    vec![],
                )],
            )];
            doc.states = vec![props(vec![(
                "items",
                Value::List(vec![record("a", "Alpha"), record("b", "Beta")]),
            )])];
            let (source, templates, controls) = doc.render_scene().unwrap();
            let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
            runtime.set_binding_value(0, "edited-a", true).unwrap();
            let items = match mode {
                0 => vec![record("b", "Beta"), record("a", "Alpha")],
                1 => vec![record("b", "Beta")],
                2 => vec![
                    record("c", "New"),
                    record("b", "model-b"),
                    record("a", "Alpha"),
                ],
                _ => vec![record("b", "Beta"), record("b", "Duplicate")],
            };
            doc.states[0].insert("items".into(), Value::List(items));
            let before = doc.states.clone();
            let result = if cached {
                doc.sync_runtime_bindings_from_scene(&runtime, None, &controls)
            } else {
                doc.sync_runtime_bindings(&runtime, None)
            };
            if mode == 3 {
                assert!(result.unwrap_err().contains("Duplicate collection"));
                assert_eq!(doc.states, before);
                continue;
            }
            result.unwrap();
            let Value::List(items) = &doc.states[0]["items"] else {
                panic!("items");
            };
            for item in items {
                let Value::Object(item) = item else {
                    panic!("record");
                };
                let expected = match item["id"].text().unwrap().as_str() {
                    "a" => "edited-a",
                    "b" => {
                        if mode == 2 {
                            "model-b"
                        } else {
                            "Beta"
                        }
                    }
                    "c" => "New",
                    _ => unreachable!(),
                };
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
        assert!(
            doc.sync_runtime_bindings_from_scene(&runtime, None, &ambiguous)
                .unwrap_err()
                .contains("Ambiguous binding")
        );
        assert_eq!(doc.states, before);
        let source = source.replace("key:'b'", "key:'a'");
        assert!(
            crate::Runtime::from_sources(&source, &templates)
                .err()
                .unwrap()
                .contains("Duplicate control key")
        );
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
        let mut doc = document(root(vec![node(
            "Control",
            vec![
                ("value", expr("state.level")),
                (
                    "__bindings",
                    Value::Object(props(vec![("value", string("state.level"))])),
                ),
            ],
            vec![],
        )]));
        doc.states = vec![props(vec![("level", number(0.5))])];
        let definition = doc.definitions.get_mut("Control").unwrap();
        definition.defaults.insert("value".into(), number(0.5));
        let range = node(
            "RangeInput",
            vec![("value", expr("props.value"))],
            vec![
                node(
                    "Minimum",
                    vec![],
                    vec![node(
                        "ContentShape",
                        vec![
                            ("points", string("0 0 10 0 10 10")),
                            ("color", string("#ffffff")),
                        ],
                        vec![],
                    )],
                ),
                node(
                    "Maximum",
                    vec![],
                    vec![node(
                        "ContentShape",
                        vec![
                            ("points", string("0 0 80 0 80 10")),
                            ("color", string("#ffffff")),
                        ],
                        vec![],
                    )],
                ),
            ],
        );
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
        let state = props(vec![(
            "items",
            Value::List(vec![
                Value::Object(props(vec![("id", string("a"))])),
                Value::Object(props(vec![("id", string("b"))])),
            ]),
        )]);
        let mut doc = Document::from_ui_files(&files, "App.ui", &state).unwrap();
        let (source, templates, _) = doc.render_scene().unwrap();
        let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
        let before = doc.states.clone();
        runtime.set_binding_value(0, "b", true).unwrap();
        assert!(
            doc.sync_runtime_bindings(&runtime, None)
                .unwrap_err()
                .contains("Duplicate collection")
        );
        assert_eq!(
            doc.states, before,
            "later lookup must not use the pre-write key index"
        );
        runtime.set_binding_value(0, "a-new", true).unwrap();
        assert_eq!(doc.sync_runtime_bindings(&runtime, None).unwrap(), 1);
        let Value::List(items) = &doc.states[0]["items"] else {
            panic!("items");
        };
        let Value::Object(first) = &items[0] else {
            panic!("item");
        };
        assert_eq!(first["id"], string("a-new"));
    }
    #[test]
    fn collection_indices_handle_distinct_outer_local_scopes() {
        for groups in [2, 10] {
            let files = BTreeMap::from([("App.ui".to_string(),
            "component App { state items: List = []; state groups: List = []; Column { width:260; height:220; for outer in state.groups key outer.id { for item in state.items key outer.id + item.id { TextInput { width:200; height:32; value <-> item.title; } } } } }".to_string())]);
            let state = props(vec![
                (
                    "items",
                    Value::List(vec![record("a", "Alpha"), record("b", "Beta")]),
                ),
                (
                    "groups",
                    Value::List(
                        (0..groups)
                            .map(|i| record(&format!("g{i}"), "Group"))
                            .collect(),
                    ),
                ),
            ]);
            let mut doc = Document::from_ui_files(&files, "App.ui", &state).unwrap();
            let (source, templates, controls) = doc.render_scene().unwrap();
            let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
            assert_eq!(runtime.control_count(), groups * 2);
            runtime
                .set_binding_value(groups * 2 - 1, "edited-b", true)
                .unwrap();
            assert_eq!(doc.sync_runtime_bindings(&runtime, None).unwrap(), 1);
            let Value::List(items) = &doc.states[0]["items"] else {
                panic!("items");
            };
            let Value::Object(second) = &items[1] else {
                panic!("item");
            };
            assert_eq!(second["title"], string("edited-b"));
            if groups == 2 {
                doc.states[0].insert(
                    "items".into(),
                    Value::List(vec![record("x", "Oversized"); 16_385]),
                );
                assert!(
                    resolve_binding_item(&controls[0], "item", &doc.states[0])
                        .unwrap_err()
                        .contains("16384 items")
                );
            }
        }
    }
    #[test]
    fn unchanged_virtual_collection_member_does_not_become_a_model_write() {
        let files = BTreeMap::from([("App.ui".to_string(),
            "component App { state items: List = []; Column { width:260; height:100; for item in state.items key item.id { TextInput { width:200; height:32; value:'5'; } } } }".to_string())]);
        let state = props(vec![("items", Value::List(vec![record("a", "Alpha")]))]);
        let mut doc = Document::from_ui_files(&files, "App.ui", &state).unwrap();
        doc.root.children[0].children[0].props.insert(
            "__bindings".into(),
            Value::Object(props(vec![("value", string("item.title.length"))])),
        );
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
                    let initial_items =
                        Value::List(vec![record("a", "Alpha"), record("b", "Beta")]);
                    let mut doc = document(root(vec![
                        node(
                            "Let",
                            vec![
                                ("name", string("rows")),
                                (
                                    "value",
                                    if derived {
                                        initial_items.clone()
                                    } else {
                                        expr("state.items")
                                    },
                                ),
                            ],
                            vec![],
                        ),
                        node(
                            "Let",
                            vec![("name", string("chosen")), ("value", expr("rows"))],
                            vec![],
                        ),
                        node(
                            "For",
                            vec![
                                ("item", string("item")),
                                ("collection", expr(collection)),
                                ("key", expr("item.id")),
                            ],
                            vec![node(
                                "Control",
                                vec![
                                    ("value", expr("item.title")),
                                    (
                                        "__bindings",
                                        Value::Object(props(vec![("value", string("item.title"))])),
                                    ),
                                ],
                                vec![],
                            )],
                        ),
                    ]));
                    let definition = doc.definitions.get_mut("Control").unwrap();
                    definition.defaults.insert("value".into(), string(""));
                    definition.nodes = vec![node(
                        "Rectangle",
                        vec![],
                        vec![node(
                            "TextInput",
                            vec![("value", expr("props.value"))],
                            vec![],
                        )],
                    )];
                    doc.states = vec![props(vec![
                        ("items", initial_items),
                        ("chosen", Value::List(vec![record("other", "Model shadow")])),
                    ])];
                    let (source, templates, controls) = doc.render_scene().unwrap();
                    let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
                    runtime.set_binding_value(0, "Edited", true).unwrap();
                    let Value::List(items) = doc.states[0].get_mut("items").unwrap() else {
                        panic!("items")
                    };
                    items.reverse();
                    let changed = if cached {
                        doc.sync_runtime_bindings_from_scene(&runtime, None, &controls)
                    } else {
                        doc.sync_runtime_bindings(&runtime, None)
                    };
                    assert_eq!(changed.unwrap(), usize::from(!derived));
                    assert_eq!(
                        lookup(&doc.states[0], "items.1.title"),
                        Some(string(if derived { "Alpha" } else { "Edited" }))
                    );
                    assert_eq!(
                        lookup(&doc.states[0], "items.0.title"),
                        Some(string("Beta"))
                    );
                    assert_eq!(
                        lookup(&doc.states[0], "chosen.0.title"),
                        Some(string("Model shadow"))
                    );
                }
            }
        }
    }
    #[test]
    fn failed_let_replay_rolls_back_earlier_widget_writes_and_retries() {
        for cached in [false, true] {
            let editor = |value: Value, path: &str| {
                node(
                    "Control",
                    vec![
                        ("value", value),
                        (
                            "__bindings",
                            Value::Object(props(vec![("value", string(path))])),
                        ),
                    ],
                    vec![],
                )
            };
            let mut doc = document(root(vec![
                editor(expr("state.query"), "state.query"),
                node(
                    "Let",
                    vec![("name", string("prefix")), ("value", expr("state.prefix"))],
                    vec![],
                ),
                node(
                    "For",
                    vec![
                        ("item", string("item")),
                        ("collection", expr("state.items")),
                        (
                            "key",
                            Value::Expression(Box::new(Expression::Binary(
                                "+".into(),
                                expr("prefix"),
                                expr("item.id"),
                            ))),
                        ),
                    ],
                    vec![editor(expr("item.title"), "item.title")],
                ),
            ]));
            let definition = doc.definitions.get_mut("Control").unwrap();
            definition.defaults.insert("value".into(), string(""));
            definition.nodes = vec![node(
                "Rectangle",
                vec![],
                vec![node(
                    "TextInput",
                    vec![("value", expr("props.value"))],
                    vec![],
                )],
            )];
            doc.states = vec![props(vec![
                ("query", string("seed")),
                ("prefix", string("p/")),
                ("items", Value::List(vec![record("a", "Alpha")])),
            ])];
            let (source, templates, controls) = doc.render_scene().unwrap();
            let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
            runtime.set_binding_value(0, "Edited query", true).unwrap();
            runtime.set_binding_value(1, "Edited item", true).unwrap();
            doc.states[0].remove("prefix");
            let before = doc.states.clone();
            for _ in 0..2 {
                let result = if cached {
                    doc.sync_runtime_bindings_from_scene(&runtime, None, &controls)
                } else {
                    doc.sync_runtime_bindings(&runtime, None)
                };
                assert!(result.unwrap_err().contains("Missing state.prefix"));
                assert_eq!(doc.states, before, "earlier editor write must roll back");
            }
            doc.states[0].insert("prefix".into(), string("p/"));
            let mut corrupt = controls.clone();
            let Some(Value::Object(active)) = corrupt[1].state.get_mut("__binding_active") else {
                panic!("active frames")
            };
            active.insert("item".into(), number(-1.));
            let before = doc.states.clone();
            assert!(
                doc.sync_runtime_bindings_from_scene(&runtime, None, &corrupt)
                    .unwrap_err()
                    .contains("Invalid active binding frame")
            );
            assert_eq!(
                doc.states, before,
                "invalid frame must roll back the earlier editor write"
            );
            assert_eq!(
                doc.sync_runtime_bindings_from_scene(&runtime, None, &controls)
                    .unwrap(),
                2
            );
            assert_eq!(doc.states[0]["query"], string("Edited query"));
            assert_eq!(
                lookup(&doc.states[0], "items.0.title"),
                Some(string("Edited item"))
            );
            assert_eq!(
                doc.sync_runtime_bindings_from_scene(&runtime, None, &controls)
                    .unwrap(),
                0
            );
            doc.states[0].insert("query".into(), string("Model query"));
            assert_eq!(
                doc.sync_runtime_bindings_from_scene(&runtime, None, &controls)
                    .unwrap(),
                0
            );
            assert_eq!(doc.states[0]["query"], string("Model query"));
        }
    }
    #[test]
    fn let_key_local_shadows_root_model_during_binding_recovery() {
        for dynamic in [false, true] {
            for cached in [false, true] {
                let mut doc = document(root(vec![
                    node(
                        "Let",
                        vec![
                            ("name", string("prefix")),
                            (
                                "value",
                                if dynamic {
                                    expr("state.prefix")
                                } else {
                                    string("lexical/")
                                },
                            ),
                        ],
                        vec![],
                    ),
                    node(
                        "Let",
                        vec![("name", string("suffix")), ("value", expr("prefix"))],
                        vec![],
                    ),
                    node(
                        "For",
                        vec![
                            ("item", string("item")),
                            ("collection", expr("state.items")),
                            (
                                "key",
                                Value::Expression(Box::new(Expression::Binary(
                                    "+".into(),
                                    expr("suffix"),
                                    expr("item.id"),
                                ))),
                            ),
                        ],
                        vec![node(
                            "Control",
                            vec![
                                ("value", expr("item.title")),
                                (
                                    "__bindings",
                                    Value::Object(props(vec![("value", string("item.title"))])),
                                ),
                            ],
                            vec![],
                        )],
                    ),
                ]));
                let definition = doc.definitions.get_mut("Control").unwrap();
                definition.defaults.insert("value".into(), string(""));
                definition.nodes = vec![node(
                    "Rectangle",
                    vec![],
                    vec![node(
                        "TextInput",
                        vec![("value", expr("props.value"))],
                        vec![],
                    )],
                )];
                doc.states = vec![props(vec![
                    ("prefix", string("model/")),
                    (
                        "items",
                        Value::List(vec![record("a", "Alpha"), record("b", "Beta")]),
                    ),
                ])];
                let (source, templates, controls) = doc.render_scene().unwrap();
                let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
                runtime.set_binding_value(0, "Edited", true).unwrap();
                doc.states[0].insert("prefix".into(), string("new-model/"));
                let Value::List(items) = doc.states[0].get_mut("items").unwrap() else {
                    panic!("items")
                };
                items.reverse();
                let changed = if cached {
                    doc.sync_runtime_bindings_from_scene(&runtime, None, &controls)
                } else {
                    doc.sync_runtime_bindings(&runtime, None)
                };
                assert_eq!(changed.unwrap(), usize::from(!dynamic));
                assert_eq!(
                    lookup(&doc.states[0], "items.1.title"),
                    Some(string(if dynamic { "Alpha" } else { "Edited" }))
                );
                assert_eq!(doc.states[0]["prefix"], string("new-model/"));
            }
        }
    }
    #[test]
    fn lexical_parent_frames_must_point_to_earlier_frames() {
        let frames = vec![Value::Object(props(vec![
            ("__item", string("item")),
            ("__collection.item", string("state.items")),
            (
                "__parents",
                Value::Object(props(vec![("item", number(0.))])),
            ),
        ]))];
        assert!(
            resolve_binding_frame(
                &frames,
                0,
                &Properties::new(),
                &mut BindingKeyIndexes::new(),
                &mut BTreeMap::new(),
                0
            )
            .unwrap_err()
            .contains("parent frame")
        );
    }
    #[test]
    fn later_loop_can_use_earlier_local_after_an_unrelated_shadow() {
        for cached in [false, true] {
            let files = BTreeMap::from([("App.ui".into(),
                "component App { state groups: List = []; state other: List = []; Column { width:260; height:400; for item in state.groups key item.id { for branch in item.items key branch.id { for item in state.other key item.id { for leaf in branch.items key item.id + leaf.id { TextInput { width:200; height:32; value <-> leaf.title; } } } } } } }".into())]);
            let group = |id: &str, children| {
                Value::Object(props(vec![
                    ("id", string(id)),
                    ("items", Value::List(children)),
                ]))
            };
            let state = props(vec![
                (
                    "groups",
                    Value::List(vec![
                        group("g1", vec![group("branch", vec![record("leaf", "First")])]),
                        group("g2", vec![group("branch", vec![record("leaf", "Second")])]),
                    ]),
                ),
                ("other", Value::List(vec![record("inner", "Unused")])),
            ]);
            let mut doc = Document::from_ui_files(&files, "App.ui", &state).unwrap();
            let (source, templates, controls) = doc.render_scene().unwrap();
            let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
            runtime.set_binding_value(0, "Edited First", true).unwrap();
            let Value::List(groups) = doc.states[0].get_mut("groups").unwrap() else {
                panic!("groups")
            };
            groups.reverse();
            let changed = if cached {
                doc.sync_runtime_bindings_from_scene(&runtime, None, &controls)
            } else {
                doc.sync_runtime_bindings(&runtime, None)
            };
            assert_eq!(changed.unwrap(), 1);
            assert_eq!(
                lookup(&doc.states[0], "groups.1.items.0.items.0.title"),
                Some(string("Edited First"))
            );
            assert_eq!(
                lookup(&doc.states[0], "groups.0.items.0.items.0.title"),
                Some(string("Second"))
            );
        }
    }
    #[test]
    fn three_shadowed_for_levels_preserve_parent_identity() {
        for middle in ["item", "branch"] {
            for cached in [false, true] {
                let source = "component App { state groups: List = []; Column { width:260; height:400; for item in state.groups key item.id { for middle in item.items key middle.id { for item in middle.items key item.id { TextInput { width:200; height:32; value <-> item.title; } } } } } }".replace("middle", middle);
                let files = BTreeMap::from([("App.ui".into(), source)]);
                let group = |id: &str, children| {
                    Value::Object(props(vec![
                        ("id", string(id)),
                        ("items", Value::List(children)),
                    ]))
                };
                let state = props(vec![(
                    "groups",
                    Value::List(vec![
                        group("root", vec![group("mid", vec![record("leaf", "Before")])]),
                        group(
                            "other",
                            vec![group("mid", vec![record("leaf", "Unchanged")])],
                        ),
                    ]),
                )]);
                let mut doc = Document::from_ui_files(&files, "App.ui", &state).unwrap();
                let (source, templates, controls) = doc.render_scene().unwrap();
                let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
                runtime.set_binding_value(0, "After", true).unwrap();
                let Value::List(groups) = doc.states[0].get_mut("groups").unwrap() else {
                    panic!("groups")
                };
                groups.reverse();
                let changed = if cached {
                    doc.sync_runtime_bindings_from_scene(&runtime, None, &controls)
                } else {
                    doc.sync_runtime_bindings(&runtime, None)
                };
                assert_eq!(changed.unwrap(), 1);
                assert_eq!(
                    lookup(&doc.states[0], "groups.1.items.0.items.0.title"),
                    Some(string("After"))
                );
                assert_eq!(
                    lookup(&doc.states[0], "groups.0.items.0.items.0.title"),
                    Some(string("Unchanged"))
                );
            }
        }
    }
    #[test]
    fn cyclic_collection_parent_metadata_refuses_before_recursing_unbounded() {
        let control = SceneControl {
            node: 0,
            key: String::new(),
            event_args: BTreeMap::new(),
            state: props(vec![("__collection.item", string("item.items"))]),
        };
        assert!(
            resolve_binding_item(&control, "item", &Properties::new())
                .unwrap_err()
                .contains("parent depth")
        );
    }
    #[test]
    fn source_scroll_viewport_extents_preserve_frame_and_clip_input() {
        for (width, height, padding, expected) in [
            ("320", "300", "0", [20., 50., 320., 300.]),
            ("50%", "50%", "0", [90., 100., 180., 200.]),
            ("50%", "50%", "20", [100., 110., 160., 180.]),
        ] {
            let source = format!(
                "component App {{ state items: List = []; Column {{ width:360; height:400; padding:{padding}; Scroll {{ width:{width}; height:{height}; Column {{ width:100; height:512; for item in state.items key item.id {{ TextInput {{ width:100; height:32; value <-> item.title; }} }} }} }} }} }}"
            );
            let files = BTreeMap::from([("App.ui".into(), source)]);
            let state = props(vec![(
                "items",
                Value::List((0..16).map(|i| record(&format!("id{i}"), "Row")).collect()),
            )]);
            let mut runtime = crate::Runtime::from_ui_files(&files, "App.ui", &state).unwrap();
            assert!(runtime.scrollable());
            assert_eq!(runtime.viewport(), expected.map(|v| v as f32).to_vec());
            assert_eq!(runtime.render_width(), 360.);
            assert_eq!(runtime.render_height(), 400.);
            runtime.scroll(0., 10_000.);
            assert_eq!(runtime.scroll_offset()[1], 512. - expected[3] as f32);
            runtime.pointer(
                expected[0] as f32 + expected[2] as f32 / 2.,
                expected[1] as f32 - 1.,
                0,
            );
            assert!((0..16).all(|i| !runtime.control_hovered(i)));
            runtime.pointer(
                expected[0] as f32 + expected[2] as f32 / 2.,
                expected[1] as f32 + 10.,
                0,
            );
            assert!((0..16).any(|i| runtime.control_hovered(i)));
        }
        let files = BTreeMap::from([("App.ui".into(),
            "component App { Column { width:360; height:400; Scroll { width:500; TextInput { width:100; height:32; } } } }".into())]);
        let doc = Document::from_ui_files(&files, "App.ui", &Properties::new()).unwrap();
        assert!(
            doc.render_scene()
                .unwrap_err()
                .contains("viewport must fit")
        );
    }
    #[test]
    fn empty_scroll_layout_retains_logical_content_height() {
        for (content, expected) in [(512, 212.), (900, 600.), (200, 0.)] {
            let source = format!(
                "component App {{ Column {{ width:360; height:400; Scroll {{ width:320; height:300; Column {{ width:100; height:{content}; }} }} }} }}"
            );
            let files = BTreeMap::from([("App.ui".into(), source)]);
            let mut runtime =
                crate::Runtime::from_ui_files(&files, "App.ui", &Properties::new()).unwrap();
            assert_eq!(runtime.control_count(), 0);
            runtime.scroll(0., 10_000.);
            assert_eq!(runtime.scroll_offset()[1], expected);
            assert_eq!(runtime.viewport(), vec![20., 50., 320., 300.]);
        }
    }

    #[test]
    fn structural_binding_maps_match_rendered_maps_across_layouts_and_branches() {
        let body = "let prefix = 'p/'; if state.enabled { for item in state.items key prefix + item.id { TextInput { width:100; height:32; value <-> item.title; } } } else { TextInput { width:100; height:32; value <-> state.query; } } Text { text:state.marker; }";
        let mut layouts: Vec<String> = ["Frame", "Column", "Row", "Grid", "Stack"]
            .iter()
            .map(|kind| format!("{kind} {{ width:360; height:400; {body} }}"))
            .collect();
        layouts.push(format!("Column {{ width:360; height:400; Scroll {{ width:360; height:300; Column {{ width:320; height:600; {body} }} }} }}"));
        for layout in layouts {
            let source = format!(
                "component App {{ state enabled: Bool = true; state query: String = 'seed'; state marker: String = 'snapshot'; state items: List = []; {layout} }}"
            );
            let files = BTreeMap::from([("App.ui".into(), source)]);
            let state = props(vec![(
                "items",
                Value::List(vec![record("a", "Alpha"), record("b", "Beta")]),
            )]);
            let mut doc = Document::from_ui_files(&files, "App.ui", &state).unwrap();
            fn annotate_events(node: &mut Node) {
                if node.kind == "TextInput" {
                    node.props.insert(
                        "__event_args".into(),
                        Value::Object(props(vec![(
                            "snapshot",
                            Value::List(vec![expr("state.marker")]),
                        )])),
                    );
                }
                for child in &mut node.children {
                    annotate_events(child);
                }
            }
            annotate_events(&mut doc.root);
            for enabled in [true, false, true] {
                doc.states[0].insert("enabled".into(), Value::Bool(enabled));
                let Value::List(items) = doc.states[0].get_mut("items").unwrap() else {
                    panic!("items")
                };
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
                assert_eq!(
                    structural[0].event_args["snapshot"],
                    vec![string("snapshot")]
                );
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
        runtime
            .set_binding_value(0, "Old branch input", true)
            .unwrap();
        doc.states[0].insert("enabled".into(), Value::Bool(false));
        assert_eq!(doc.sync_runtime_bindings(&runtime, None).unwrap(), 0);
        assert_eq!(
            lookup(&doc.states[0], "items.0.title"),
            Some(string("Alpha"))
        );
        assert_eq!(doc.states[0]["other"], string("Other"));
        let (source, templates, fallback) = doc.render_scene().unwrap();
        assert_ne!(first[0].key, fallback[0].key);
        assert_eq!(fallback[0].state["suffix"], string("fallback"));
        assert!(!fallback[1].state.contains_key("suffix"));
        let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
        runtime
            .set_binding_value(0, "Edited fallback", true)
            .unwrap();
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
        let state = props(vec![(
            "items",
            Value::List(vec![record("a", "Alpha"), record("b", "Beta")]),
        )]);
        let mut doc = Document::from_ui_files(&files, "App.ui", &state).unwrap();
        let (_, _, first) = doc.render_scene().unwrap();
        assert_eq!(first[0].state["prefix"], string("a"));
        assert_eq!(first[1].state["prefix"], string("b"));
        assert_eq!(first[2].state["prefix"], string("root/"));
        let Value::List(items) = doc.states[0].get_mut("items").unwrap() else {
            panic!("items")
        };
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
                            *source = source.replace(
                                &format!("for item in {parent}.items"),
                                &format!("let rows = {parent}.items; for item in rows"),
                            );
                        }
                        let group = |id: &str, children| {
                            Value::Object(props(vec![
                                ("id", string(id)),
                                ("items", Value::List(children)),
                            ]))
                        };
                        let initial = props(vec![(
                            "groups",
                            Value::List(vec![
                                group("g1", vec![record("a", "Alpha"), record("b", "Beta")]),
                                group("g2", vec![record("a", "Other Alpha")]),
                            ]),
                        )]);
                        let mut doc = Document::from_ui_files(&files, "App.ui", &initial).unwrap();
                        let (source, templates, controls) = doc.render_scene().unwrap();
                        let mut runtime =
                            crate::Runtime::from_sources(&source, &templates).unwrap();
                        runtime.set_binding_value(0, "edited-g1-a", true).unwrap();
                        let groups = match mode {
                            0 => vec![
                                group("g2", vec![record("a", "Other Alpha")]),
                                group("g1", vec![record("b", "Beta"), record("a", "Alpha")]),
                            ],
                            1 => vec![group("g2", vec![record("a", "Other Alpha")])],
                            _ => vec![
                                group("g1", vec![record("a", "Alpha")]),
                                group("g1", vec![record("a", "Duplicate")]),
                            ],
                        };
                        doc.states[0].insert("groups".into(), Value::List(groups));
                        let before = doc.states.clone();
                        let result = if cached {
                            doc.sync_runtime_bindings_from_scene(&runtime, None, &controls)
                        } else {
                            doc.sync_runtime_bindings(&runtime, None)
                        };
                        if mode == 2 {
                            assert!(result.unwrap_err().contains("Duplicate collection"));
                            assert_eq!(doc.states, before);
                        } else {
                            assert_eq!(result.unwrap(), usize::from(mode == 0));
                            assert_eq!(
                                lookup(&doc.states[0], "groups.0.items.0.title"),
                                Some(string("Other Alpha"))
                            );
                            if mode == 0 {
                                assert_eq!(
                                    lookup(&doc.states[0], "groups.1.items.1.title"),
                                    Some(string("edited-g1-a"))
                                );
                                assert_eq!(
                                    lookup(&doc.states[0], "groups.1.items.0.title"),
                                    Some(string("Beta"))
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn nested_list_paths_read_and_write_without_replacing_lists() {
        let mut state = props(vec![(
            "groups",
            Value::List(vec![Value::Object(props(vec![(
                "items",
                Value::List(vec![record("a", "Alpha"), record("b", "Beta")]),
            )]))]),
        )]);
        assert_eq!(
            lookup(&state, "groups.0.items.1.title"),
            Some(string("Beta"))
        );
        assert_eq!(
            lookup_binding_ref(&state, "groups.0.items.1.title"),
            Some(&string("Beta"))
        );
        assert_eq!(lookup(&state, "groups.0.items.length"), Some(number(2.)));
        assert!(lookup_binding_ref(&state, "groups.0.items.length").is_none());
        assert!(write_collection_item_path(
            &mut state,
            "groups.0.items",
            1,
            "title",
            string("Edited")
        ));
        assert_eq!(
            lookup(&state, "groups.0.items.1.title"),
            Some(string("Edited"))
        );
        assert!(write_state_path(
            &mut state,
            "groups.0.items.0.title",
            string("First")
        ));
        let before = state.clone();
        for path in [
            "groups.9.items.0.title",
            "groups.bad.items",
            "groups.0.items.length",
        ] {
            assert!(!write_state_path(&mut state, path, string("invalid")));
            assert_eq!(state, before);
        }
    }
    #[test]
    fn bindings_route_root_backed_collections_through_list_indices() {
        let mut doc = document(root(vec![node(
            "For",
            vec![
                ("item", string("item")),
                ("collection", expr("state.groups.0.items")),
                ("key", expr("item.id")),
            ],
            vec![node(
                "Control",
                vec![
                    ("value", expr("item.title")),
                    (
                        "__bindings",
                        Value::Object(props(vec![("value", string("item.title"))])),
                    ),
                ],
                vec![],
            )],
        )]));
        let definition = doc.definitions.get_mut("Control").unwrap();
        definition.defaults.insert("value".into(), string(""));
        definition.nodes = vec![node(
            "Rectangle",
            vec![],
            vec![node(
                "TextInput",
                vec![("value", expr("props.value"))],
                vec![],
            )],
        )];
        doc.states = vec![props(vec![(
            "groups",
            Value::List(vec![Value::Object(props(vec![(
                "items",
                Value::List(vec![record("a", "Alpha"), record("b", "Beta")]),
            )]))]),
        )])];
        let (source, templates, controls) = doc.render_scene().unwrap();
        let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
        runtime.set_binding_value(0, "edited-a", true).unwrap();
        let Some(Value::List(items)) = lookup_binding_mut(&mut doc.states[0], "groups.0.items")
        else {
            panic!("items")
        };
        items.reverse();
        assert_eq!(
            doc.sync_runtime_bindings_from_scene(&runtime, None, &controls)
                .unwrap(),
            1
        );
        assert_eq!(
            lookup(&doc.states[0], "groups.0.items.1.title"),
            Some(string("edited-a"))
        );
        assert_eq!(
            lookup(&doc.states[0], "groups.0.items.0.title"),
            Some(string("Beta"))
        );
    }
    #[test]
    fn collection_key_encoding_preserves_types_and_utf8_lengths() {
        for (value, expected) in [
            (string(""), "s0:"),
            (string("я🙂"), "s6:я🙂"),
            (string("12"), "s2:12"),
            (number(12.), "n2:12"),
            (number(-1.5), "n4:-1.5"),
        ] {
            assert_eq!(key_part(&value).unwrap(), expected);
        }
        for value in [
            number(f64::NAN),
            number(f64::INFINITY),
            Value::Bool(true),
            Value::Null,
        ] {
            assert!(key_part(&value).is_err());
        }
    }
    #[test]
    fn borrowed_collection_keys_preserve_computed_and_exact_field_semantics() {
        let item =
            |id: Value, raw: &str| Value::Object(props(vec![("id", id), ("raw", string(raw))]));
        let mut state = props(vec![(
            "items",
            Value::List(vec![
                item(string("a"), "unused"),
                item(expr("item.raw"), "b"),
            ]),
        )]);
        let control = SceneControl {
            node: 0,
            key: String::new(),
            event_args: BTreeMap::new(),
            state: props(vec![
                ("__collection.item", string("state.items")),
                (
                    "__binding_key.item",
                    string(&key_part(&string("b")).unwrap()),
                ),
                ("__binding_key_expr.item", expr("item.id")),
            ]),
        };
        assert_eq!(
            resolve_binding_item(&control, "item", &state).unwrap(),
            Some(("items".into(), 1))
        );
        // Exact dotted root fields take precedence over item members, causing
        // duplicate keys here; the borrowed path must not bypass this refusal.
        state.insert("item.id".into(), string("override"));
        assert!(
            resolve_binding_item(&control, "item", &state)
                .unwrap_err()
                .contains("Duplicate")
        );
        state.remove("item.id");
        let Value::List(items) = state.get_mut("items").unwrap() else {
            panic!("items")
        };
        items.reverse();
        assert_eq!(
            resolve_binding_item(&control, "item", &state).unwrap(),
            Some(("items".into(), 0))
        );
    }
    #[test]
    fn reused_for_scope_preserves_nested_shadowing_and_owned_snapshots() {
        let control = || node("Control", vec![("text", expr("item.title"))], vec![]);
        let loop_node = |collection: &str, children| {
            node(
                "For",
                vec![
                    ("item", string("item")),
                    ("collection", expr(collection)),
                    ("key", expr("item.id")),
                ],
                children,
            )
        };
        let mut doc = document(root(vec![loop_node(
            "state.items",
            vec![loop_node("state.inner", vec![control()]), control()],
        )]));
        doc.states = vec![props(vec![
            (
                "items",
                Value::List(vec![record("a", "Alpha"), record("b", "Beta")]),
            ),
            (
                "inner",
                Value::List(vec![record("x", "Ex"), record("y", "Why")]),
            ),
        ])];
        let (_, _, snapshots) = doc.render_scene().unwrap();
        let titles: Vec<_> = snapshots
            .iter()
            .map(|c| {
                let Value::Object(item) = &c.state["item"] else {
                    panic!("item snapshot")
                };
                item["title"].text().unwrap()
            })
            .collect();
        assert_eq!(titles, ["Ex", "Why", "Alpha", "Ex", "Why", "Beta"]);
        for (i, key) in ["x", "y", "a", "x", "y", "b"].iter().enumerate() {
            assert_eq!(
                snapshots[i].state["__binding_key.item"],
                string(&key_part(&string(key)).unwrap())
            );
        }
        doc.states[0].insert("items".into(), Value::List(vec![record("c", "Changed")]));
        let (_, _, next) = doc.render_scene().unwrap();
        assert_eq!(next.len(), 3);
        assert_eq!(
            snapshots[5].state["__binding_key.item"],
            string(&key_part(&string("b")).unwrap())
        );
        assert!(!doc.states[0].contains_key("item"));
    }
    #[test]
    fn borrowed_root_alias_keys_match_general_replay_on_reorder_and_key_errors() {
        let files = BTreeMap::from([("App.ui".into(), "component App { state items: List = []; Column { let visible = state.items; let rows = visible; for item in rows key item.id { TextInput { width:100; height:30; value <-> item.title; } } } }".into())]);
        let item = |id: Value, title: &str| {
            Value::Object(props(vec![("id", id), ("title", string(title))]))
        };
        let mut doc = Document::from_ui_files(
            &files,
            "App.ui",
            &props(vec![(
                "items",
                Value::List(vec![item(string("a"), "Alpha"), item(string("b"), "Beta")]),
            )]),
        )
        .unwrap();
        let (_, _, controls) = doc.render_scene().unwrap();
        let mut reference = controls[0].clone();
        let Value::List(frames) = reference.state.get_mut("__binding_frames").unwrap() else {
            panic!("frames")
        };
        let Value::Object(frame) = &mut frames[0] else {
            panic!("frame")
        };
        let Value::List(steps) = frame.get_mut("__let_steps").unwrap() else {
            panic!("steps")
        };
        // An unrelated literal Let requires general replay without changing the key.
        steps.push(Value::Object(props(vec![
            ("name", string("unused_marker")),
            ("expression", string("marker")),
        ])));
        let Value::Object(lexicals) = frame.get_mut("__lexicals").unwrap() else {
            panic!("lexicals")
        };
        lexicals.insert("unused_marker".into(), string("marker"));
        let compare = |state: &Properties| {
            let actual = resolve_binding_item(&controls[0], "item", state);
            let expected = resolve_binding_item(&reference, "item", state);
            assert_eq!(actual, expected);
            actual
        };
        assert_eq!(compare(&doc.states[0]).unwrap(), Some(("items".into(), 0)));
        let Value::List(items) = doc.states[0].get_mut("items").unwrap() else {
            panic!("items")
        };
        items.reverse();
        assert_eq!(compare(&doc.states[0]).unwrap(), Some(("items".into(), 1)));
        doc.states[0].insert("item.id".into(), string("override"));
        assert!(compare(&doc.states[0]).unwrap_err().contains("Duplicate"));
        doc.states[0].remove("item.id");
        let Value::List(items) = doc.states[0].get_mut("items").unwrap() else {
            panic!("items")
        };
        let Value::Object(first) = &mut items[0] else {
            panic!("item")
        };
        first.insert("id".into(), expr("item.title"));
        assert_eq!(compare(&doc.states[0]).unwrap(), Some(("items".into(), 1)));
    }

    #[test]
    fn direct_alias_replay_preserves_shadowing_and_readonly_origins() {
        for (lets, target) in [
            ("let visible = state.items;", Some("items")),
            ("let rows = state.items; let visible = rows;", Some("items")),
            (
                "let items = state.other; let visible = items;",
                Some("other"),
            ),
            (
                "let visible = state.items; let items = state.other;",
                Some("items"),
            ),
            (
                "let visible = state.items; let visible = state.other;",
                Some("other"),
            ),
            (
                "let marker = 'literal'; let visible = state.items;",
                Some("items"),
            ),
            ("let visible = true ? state.items : state.other;", None),
        ] {
            let source = format!(
                "component App {{ state items: List = []; state other: List = []; Column {{ {lets} for item in visible key item.id {{ TextInput {{ width:100; height:30; value <-> item.title; }} }} }} }}"
            );
            let files = BTreeMap::from([("App.ui".into(), source)]);
            let list = |prefix: &str| {
                Value::List(
                    ["a", "b"]
                        .into_iter()
                        .map(|id| {
                            Value::Object(props(vec![
                                ("id", string(id)),
                                ("title", string(&format!("{prefix}-{id}"))),
                            ]))
                        })
                        .collect(),
                )
            };
            let mut doc = Document::from_ui_files(
                &files,
                "App.ui",
                &props(vec![("items", list("items")), ("other", list("other"))]),
            )
            .unwrap();
            let (_, _, controls) = doc.render_scene().unwrap();
            let mut reference = controls[0].clone();
            let Value::List(frames) = reference.state.get_mut("__binding_frames").unwrap() else {
                panic!("frames")
            };
            let Value::Object(frame) = &mut frames[0] else {
                panic!("frame")
            };
            let Value::List(steps) = frame.get_mut("__let_steps").unwrap() else {
                panic!("steps")
            };
            steps.push(Value::Object(props(vec![
                ("name", string("unused_reference_marker")),
                ("expression", string("marker")),
            ])));
            let Value::Object(lexicals) = frame.get_mut("__lexicals").unwrap() else {
                panic!("lexicals")
            };
            lexicals.insert("unused_reference_marker".into(), string("marker"));
            for reversed in [false, true] {
                if reversed {
                    for name in ["items", "other"] {
                        let Value::List(items) = doc.states[0].get_mut(name).unwrap() else {
                            panic!("items")
                        };
                        items.reverse();
                    }
                }
                let actual = resolve_binding_item(&controls[0], "item", &doc.states[0]);
                let expected = resolve_binding_item(&reference, "item", &doc.states[0]);
                assert_eq!(actual, expected, "{lets}");
                assert_eq!(
                    actual.unwrap(),
                    target.map(|target| (target.into(), usize::from(reversed))),
                    "{lets}"
                );
            }
        }
    }

    #[test]
    fn direct_alias_shortcut_does_not_hide_an_unused_let_replay_error() {
        let files = BTreeMap::from([("App.ui".into(), "component App { state items: List = []; state unused: List = []; Column { let ignored = state.unused; let visible = state.items; for item in visible key item.id { TextInput { width:100; height:30; value <-> item.title; } } } }".into())]);
        let mut doc = Document::from_ui_files(
            &files,
            "App.ui",
            &props(vec![(
                "items",
                Value::List(vec![Value::Object(props(vec![
                    ("id", string("a")),
                    ("title", string("Alpha")),
                ]))]),
            )]),
        )
        .unwrap();
        let (_, _, controls) = doc.render_scene().unwrap();
        doc.states[0].remove("unused");
        let before = doc.states.clone();
        assert!(
            resolve_binding_item(&controls[0], "item", &doc.states[0])
                .unwrap_err()
                .contains("Missing state.unused")
        );
        assert_eq!(doc.states, before);
    }

    #[test]
    fn later_let_metadata_is_retained_when_not_represented_in_collection_frame() {
        let files = BTreeMap::from([("App.ui".into(), "component App { state items: List = []; state suffix: String = 'tail'; Column { let visible = state.items; for item in visible key item.id { let extra = state.suffix; TextInput { width:100; height:30; value <-> item.title; } } } }".into())]);
        let state = props(vec![(
            "items",
            Value::List(vec![Value::Object(props(vec![
                ("id", string("a")),
                ("title", string("Alpha")),
            ]))]),
        )]);
        let doc = Document::from_ui_files(&files, "App.ui", &state).unwrap();
        let (_, _, controls) = doc.render_scene().unwrap();
        assert_eq!(controls.len(), 1);
        assert_eq!(controls[0].state["extra"], string("tail"));
        let Value::Object(lexicals) = &controls[0].state["__binding_lexicals"] else {
            panic!("lexicals")
        };
        assert_eq!(lexicals["extra"], string("tail"));
        assert!(controls[0].state.contains_key("__binding_let_steps"));
    }
    #[test]
    fn compact_lexical_frames_preserve_model_values_and_legacy_map_recovery() {
        let files = BTreeMap::from([("App.ui".into(), "component App { state items: List = []; Column { let visible = state.items; for item in visible key item.id { TextInput { width:100; height:30; value <-> item.title; } } } }".into())]);
        let item = |id: &str, title: &str| {
            Value::Object(props(vec![("id", string(id)), ("title", string(title))]))
        };
        let mut doc = Document::from_ui_files(
            &files,
            "App.ui",
            &props(vec![(
                "items",
                Value::List(vec![item("a", "Alpha"), item("b", "Beta")]),
            )]),
        )
        .unwrap();
        let mut legacy_doc = doc.clone();
        let (source, templates, controls) = doc.render_scene().unwrap();
        let mut legacy = controls.clone();
        for control in &mut legacy {
            let Value::List(frames) = control.state.get_mut("__binding_frames").unwrap() else {
                panic!("frames")
            };
            let Value::Object(frame) = &mut frames[0] else {
                panic!("frame")
            };
            let Value::Object(lexicals) = frame["__lexicals"].clone() else {
                panic!("lexicals")
            };
            let Value::Object(locals) = frame.get_mut("__locals").unwrap() else {
                panic!("locals")
            };
            assert!(!locals.contains_key("visible"));
            locals.extend(lexicals);
            let replay_lexicals = frame["__lexicals"].clone();
            let replay_steps = frame["__let_steps"].clone();
            control
                .state
                .insert("__binding_lexicals".into(), replay_lexicals);
            control
                .state
                .insert("__binding_let_steps".into(), replay_steps);
        }
        assert!(!controls[0].state.contains_key("__binding_lexicals"));
        assert!(!controls[0].state.contains_key("__binding_let_steps"));
        assert!(matches!(&controls[0].state["visible"], Value::List(items) if items.len() == 2));
        assert!(matches!(&controls[0].state["items"], Value::List(items) if items.len() == 2));
        let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
        runtime.set_binding_value(0, "edited", true).unwrap();
        let mut legacy_runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
        legacy_runtime.set_binding_value(0, "edited", true).unwrap();
        assert_eq!(
            doc.sync_runtime_bindings_from_scene(&runtime, None, &controls)
                .unwrap(),
            1
        );
        assert_eq!(
            legacy_doc
                .sync_runtime_bindings_from_scene(&legacy_runtime, None, &legacy)
                .unwrap(),
            1
        );
        assert_eq!(doc.states, legacy_doc.states);
    }
    #[test]
    fn computed_item_member_keys_keep_captured_props_scope_during_recovery() {
        let files = BTreeMap::from([("App.ui".into(), "component App { prop prefix: String = 'stable'; state items: List = []; Column { let visible = state.items; let chosen = visible; for item in chosen key item.id { TextInput { width:100; height:30; value <-> item.title; } } } }".into())]);
        let item = |id: Value, title: &str| {
            Value::Object(props(vec![("id", id), ("title", string(title))]))
        };
        let mut doc = Document::from_ui_files(
            &files,
            "App.ui",
            &props(vec![(
                "items",
                Value::List(vec![
                    item(expr("props.prefix"), "Alpha"),
                    item(string("other"), "Beta"),
                ]),
            )]),
        )
        .unwrap();
        let (source, templates, controls) = doc.render_scene().unwrap();
        let frames = controls[0].state["__binding_frames"].clone();
        let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
        runtime.set_binding_value(0, "edited", true).unwrap();
        let Value::List(items) = doc.states[0].get_mut("items").unwrap() else {
            panic!("items")
        };
        items.swap(0, 1);
        assert_eq!(
            doc.sync_runtime_bindings_from_scene(&runtime, None, &controls)
                .unwrap(),
            1
        );
        let Value::List(items) = &doc.states[0]["items"] else {
            panic!("items")
        };
        let Value::Object(first) = &items[0] else {
            panic!("first")
        };
        let Value::Object(second) = &items[1] else {
            panic!("second")
        };
        assert_eq!(first["title"], string("Beta"));
        assert_eq!(second["title"], string("edited"));
        assert_eq!(controls[0].state["__binding_frames"], frames);
    }
    #[test]
    fn flat_frame_owned_control_snapshots_survive_subsequent_model_replacement() {
        let files = BTreeMap::from([("App.ui".into(), "component App { state items: List = []; Frame { width:260; height:300; for item in state.items key item.id { TextInput { width:200; height:32; value <-> item.title; } } } }".into())]);
        let item = |id: &str, title: &str| {
            Value::Object(props(vec![("id", string(id)), ("title", string(title))]))
        };
        let mut doc = Document::from_ui_files(
            &files,
            "App.ui",
            &props(vec![(
                "items",
                Value::List(vec![item("a", "Alpha"), item("b", "Beta")]),
            )]),
        )
        .unwrap();
        let first = doc.render_scene_artifact().unwrap();
        assert_eq!(first.controls.len(), 2);
        doc.states[0].insert("items".into(), Value::List(vec![item("c", "Changed")]));
        let second = doc.render_scene_artifact().unwrap();
        assert_eq!(second.controls.len(), 1);
        for (control, title) in first.controls.iter().zip(["Alpha", "Beta"]) {
            let Value::Object(local) = &control.state["item"] else {
                panic!("item snapshot")
            };
            assert_eq!(local["title"], string(title));
            let Value::List(items) = &control.state["items"] else {
                panic!("root snapshot")
            };
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
        let state = props(vec![
            ("caption", string("snapshot")),
            ("items", Value::List(vec![number(1.), number(2.)])),
        ]);
        let mut instance = node(
            "Control",
            vec![
                ("text", expr("props.__state.caption")),
                ("__state", Value::Object(state.clone())),
            ],
            vec![],
        );
        let original = instance.clone();
        let expected = doc.instantiate(&instance, true, &state, &[]).unwrap();
        let actual = doc.instantiate_scene(&mut instance).unwrap();
        assert_eq!(actual, expected);
        assert_eq!(instance, original);
        doc.definitions.get_mut("Control").unwrap().nodes =
            vec![node("Text", vec![("text", string("invalid root"))], vec![])];
        assert!(
            doc.instantiate_scene(&mut instance)
                .unwrap_err()
                .contains("Rectangle root")
        );
        assert_eq!(
            instance, original,
            "failed evaluation must return the owned state snapshot"
        );
        doc.definitions.get_mut("Control").unwrap().nodes = vec![node("Rectangle", vec![], vec![])];
        doc.definitions
            .get_mut("Control")
            .unwrap()
            .defaults
            .insert("__state".into(), Value::Object(state));
        let mut missing_snapshot = node("Control", vec![("text", expr("state.caption"))], vec![]);
        assert!(
            doc.instantiate(&missing_snapshot, true, &Properties::new(), &[])
                .is_err()
        );
        assert!(doc.instantiate_scene(&mut missing_snapshot).is_err());
        assert!(
            !missing_snapshot.props.contains_key("__state"),
            "definition defaults must not become the caller state"
        );
    }
    #[test]
    fn borrowed_definition_content_is_instantiated_without_mutating_shared_templates() {
        let mut doc = document(root(vec![]));
        doc.definitions.get_mut("Control").unwrap().nodes = vec![node(
            "Rectangle",
            vec![],
            vec![node(
                "ContentPresenter",
                vec![("key", string("content"))],
                vec![],
            )],
        )];
        let original = doc.definitions["Control"].nodes.clone();
        let first = doc
            .instantiate(
                &node(
                    "Control",
                    vec![],
                    vec![node("Text", vec![("text", string("first"))], vec![])],
                ),
                true,
                &Properties::new(),
                &[],
            )
            .unwrap()
            .0;
        let second = doc
            .instantiate(
                &node(
                    "Control",
                    vec![],
                    vec![node("Text", vec![("text", string("second"))], vec![])],
                ),
                true,
                &Properties::new(),
                &[],
            )
            .unwrap()
            .0;
        assert_eq!(first.children[0].children[0].props["text"], string("first"));
        assert_eq!(
            second.children[0].children[0].props["text"],
            string("second")
        );
        assert_eq!(doc.definitions["Control"].nodes, original);
        let empty = doc
            .instantiate(
                &node("Control", vec![], vec![]),
                true,
                &Properties::new(),
                &[],
            )
            .unwrap()
            .0;
        assert!(empty.children[0].children.is_empty());
        assert_eq!(doc.definitions["Control"].nodes, original);
    }
    #[test]
    fn visual_sequence_borrowing_preserves_lexical_shadowing_and_owned_results() {
        let doc = document(root(vec![]));
        let label = || node("Text", vec![("text", expr("state.local"))], vec![]);
        let binding = |value: &str| {
            node(
                "Let",
                vec![("name", string("local")), ("value", string(value))],
                vec![],
            )
        };
        let nodes = vec![
            label(),
            binding("later"),
            label(),
            node("Frame", vec![], vec![binding("inner"), label()]),
            label(),
        ];
        let mut state = props(vec![("local", string("parent"))]);
        let expanded = doc
            .expand_sequence(&nodes, &Properties::new(), &state, &[], 0)
            .unwrap();
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
        let binding = |value: &str| {
            node(
                "Let",
                vec![("name", string("local")), ("value", string(value))],
                vec![],
            )
        };
        let mut doc = document(root(vec![
            control(),
            control(),
            binding("later"),
            control(),
            node(
                "Frame",
                vec![("width", number(180.)), ("height", number(30.))],
                vec![binding("inner"), control()],
            ),
            control(),
        ]));
        doc.states = vec![Properties::new(); 9];
        doc.states[0].insert("local".into(), string("parent"));
        doc.states[2].insert("local".into(), string("node"));
        let (_, _, snapshots) = doc.render_scene().unwrap();
        let values: Vec<_> = snapshots
            .iter()
            .map(|control| control.state["local"].text().unwrap())
            .collect();
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
    fn text_font_family_reaches_the_scene_and_sizes_by_monospace_metrics() {
        let scene = |family: &str| {
            let mut files: BTreeMap<String, String> = BTreeMap::new();
            files.insert(
                "Code.component.ui".into(),
                format!("component Code {{ width: 400; height: 60; Rectangle {{ Frame {{ columns: [-, *]; Text {{ cell: 1 1; text: 'iiii\nWWWW'; fontSize: 20; {family} }} }} }} }}"),
            );
            files.insert(
                "App.ui".into(),
                "component App { Column { width: 420; height: 80; Code { key: 'code'; } } }".into(),
            );
            let doc = Document::from_ui_files(&files, "App.ui", &Properties::new()).unwrap();
            let (scene_src, templates, _) = doc.render_scene().unwrap();
            crate::Runtime::from_sources(&scene_src, &templates).unwrap();
            templates
        };
        let widths = |templates: &str| -> Vec<f64> {
            templates
                .split("ContentText {")
                .skip(1)
                .map(|part| {
                    let rest = part.split("width:").nth(1).unwrap();
                    rest.split(';').next().unwrap().trim().parse().unwrap()
                })
                .collect()
        };
        let sans = scene("");
        assert!(!sans.contains("fontFamily"));
        let sans_widths = widths(&sans);
        assert_ne!(sans_widths[0], sans_widths[1]);

        let mono = scene("fontFamily: 'mono';");
        assert_eq!(mono.matches("fontFamily:mono;").count(), 2);
        let mono_widths = widths(&mono);
        assert_eq!(mono_widths[0], mono_widths[1]);

        let mut files: BTreeMap<String, String> = BTreeMap::new();
        files.insert(
            "App.ui".into(),
            "component App { Column { width: 200; height: 80; Text { key: 't'; text: 'x'; fontFamily: 'serif'; } } }".into(),
        );
        let bad = Document::from_ui_files(&files, "App.ui", &Properties::new())
            .and_then(|doc| doc.render_scene().map(|_| ()));
        assert!(bad.is_err());
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
            if empty {
                doc.states.clear();
            }
            let before = doc.states.clone();
            fn second_binding(node: &mut Node, count: &mut usize, value: Value) -> bool {
                if let Some(Value::Object(bindings)) = node.props.get_mut("__bindings") {
                    *count += 1;
                    if *count == 2 {
                        bindings.insert("value".into(), value);
                        return true;
                    }
                }
                node.children
                    .iter_mut()
                    .any(|node| second_binding(node, count, value.clone()))
            }
            assert!(second_binding(&mut doc.root, &mut 0, Value::List(vec![])));
            let result = if cached {
                doc.sync_runtime_bindings_from_scene(&runtime, None, &controls)
            } else {
                doc.sync_runtime_bindings(&runtime, None)
            };
            assert!(result.unwrap_err().contains("Expected scalar text"));
            assert_eq!(
                doc.states, before,
                "a later invalid binding must roll back earlier writes"
            );
            assert!(second_binding(
                &mut doc.root,
                &mut 0,
                Value::Text("state.second".into())
            ));
            let changed = if cached {
                doc.sync_runtime_bindings_from_scene(&runtime, None, &controls)
                    .unwrap()
            } else {
                doc.sync_runtime_bindings(&runtime, None).unwrap()
            };
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
        let mut nested = Node {
            kind: "Frame".into(),
            props: Properties::new(),
            children: vec![],
            action: None,
        };
        for _ in 0..66 {
            nested = Node {
                kind: "Frame".into(),
                props: Properties::new(),
                children: vec![nested],
                action: None,
            };
        }
        doc.root.children.push(nested);
        let error = doc
            .sync_runtime_bindings_from_scene(&runtime, None, &controls)
            .unwrap_err();
        assert!(error.contains("depth exceeded"));
        assert_eq!(doc.states, before);
        assert!(doc.render_scene().unwrap_err().contains("depth exceeded"));
        doc.root.children.pop();
        doc.root.children.extend((0..16_384).map(|_| Node {
            kind: "Frame".into(),
            props: Properties::new(),
            children: vec![],
            action: None,
        }));
        assert!(
            doc.sync_runtime_bindings_from_scene(&runtime, None, &controls)
                .unwrap_err()
                .contains("16384 nodes")
        );
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
