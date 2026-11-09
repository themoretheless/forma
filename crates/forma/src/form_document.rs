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
                        write_str_literal(line, out);
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
                        write_str_literal(line, out);
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
mod tests;
