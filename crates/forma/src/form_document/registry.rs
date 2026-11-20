//! Authoritative `.ui` property and binding registry: types, invalidation classes,
//! two-way binding capabilities, per-node vocabularies, and dependency analysis.

use super::{BuiltinKind, Document, Node, Properties, Value};
use std::collections::{BTreeMap, BTreeSet};

/// Static value domain of a `.ui` property.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize)]
pub enum PropertyType {
    Number,
    Length,
    Insets,
    Gaps,
    Tracks,
    Cell,
    Boolean,
    Text,
    Color,
    Duration,
    Angle,
    Asset,
    List,
    Object,
    Any,
}

/// Broadest invalidation triggered when a property changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
pub enum InvalidationClass {
    None,
    Paint,
    Layout,
    Structure,
}

impl InvalidationClass {
    pub const fn combine(self, other: Self) -> Self {
        match (self, other) {
            (Self::Structure, _) | (_, Self::Structure) => Self::Structure,
            (Self::Layout, _) | (_, Self::Layout) => Self::Layout,
            (Self::Paint, _) | (_, Self::Paint) => Self::Paint,
            (Self::None, Self::None) => Self::None,
        }
    }
}

/// How a property participates in reactive `<-` and `<->` bindings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize)]
pub enum BindingCapability {
    None,
    OneWay,
    TwoWayValue,
    TwoWayChecked,
    TwoWaySelected,
    TwoWayGroup,
}

/// Registry entry describing one canonical `.ui` property.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct PropertySpec {
    pub name: &'static str,
    pub alias: Option<&'static str>,
    pub ty: PropertyType,
    pub invalidation: InvalidationClass,
    pub binding: BindingCapability,
    pub styleable: bool,
    pub scene_control: bool,
}

const fn prop(
    name: &'static str,
    alias: Option<&'static str>,
    ty: PropertyType,
    invalidation: InvalidationClass,
    binding: BindingCapability,
    styleable: bool,
    scene_control: bool,
) -> PropertySpec {
    PropertySpec {
        name,
        alias,
        ty,
        invalidation,
        binding,
        styleable,
        scene_control,
    }
}

pub const PROPERTIES: &[PropertySpec] = &[
    prop(
        "key",
        None,
        PropertyType::Text,
        InvalidationClass::Structure,
        BindingCapability::OneWay,
        false,
        true,
    ),
    prop(
        "x",
        None,
        PropertyType::Length,
        InvalidationClass::Layout,
        BindingCapability::OneWay,
        false,
        true,
    ),
    prop(
        "y",
        None,
        PropertyType::Length,
        InvalidationClass::Layout,
        BindingCapability::OneWay,
        false,
        true,
    ),
    prop(
        "width",
        None,
        PropertyType::Length,
        InvalidationClass::Layout,
        BindingCapability::OneWay,
        true,
        true,
    ),
    prop(
        "height",
        None,
        PropertyType::Length,
        InvalidationClass::Layout,
        BindingCapability::OneWay,
        true,
        true,
    ),
    prop(
        "minWidth",
        None,
        PropertyType::Length,
        InvalidationClass::Layout,
        BindingCapability::OneWay,
        true,
        false,
    ),
    prop(
        "maxWidth",
        None,
        PropertyType::Length,
        InvalidationClass::Layout,
        BindingCapability::OneWay,
        true,
        false,
    ),
    prop(
        "minHeight",
        None,
        PropertyType::Length,
        InvalidationClass::Layout,
        BindingCapability::OneWay,
        true,
        false,
    ),
    prop(
        "maxHeight",
        None,
        PropertyType::Length,
        InvalidationClass::Layout,
        BindingCapability::OneWay,
        true,
        false,
    ),
    prop(
        "padding",
        None,
        PropertyType::Insets,
        InvalidationClass::Layout,
        BindingCapability::OneWay,
        true,
        false,
    ),
    prop(
        "gap",
        None,
        PropertyType::Gaps,
        InvalidationClass::Layout,
        BindingCapability::OneWay,
        true,
        false,
    ),
    prop(
        "columns",
        None,
        PropertyType::Tracks,
        InvalidationClass::Layout,
        BindingCapability::OneWay,
        false,
        false,
    ),
    prop(
        "rows",
        None,
        PropertyType::Tracks,
        InvalidationClass::Layout,
        BindingCapability::OneWay,
        false,
        false,
    ),
    prop(
        "cell",
        None,
        PropertyType::Cell,
        InvalidationClass::Layout,
        BindingCapability::None,
        false,
        false,
    ),
    prop(
        "row",
        None,
        PropertyType::Number,
        InvalidationClass::Layout,
        BindingCapability::None,
        false,
        false,
    ),
    prop(
        "column",
        None,
        PropertyType::Number,
        InvalidationClass::Layout,
        BindingCapability::None,
        false,
        false,
    ),
    prop(
        "row.span",
        None,
        PropertyType::Number,
        InvalidationClass::Layout,
        BindingCapability::None,
        false,
        false,
    ),
    prop(
        "column.span",
        None,
        PropertyType::Number,
        InvalidationClass::Layout,
        BindingCapability::None,
        false,
        false,
    ),
    prop(
        "text",
        None,
        PropertyType::Text,
        InvalidationClass::Paint,
        BindingCapability::OneWay,
        true,
        true,
    ),
    prop(
        "runs",
        None,
        PropertyType::List,
        InvalidationClass::Layout,
        BindingCapability::OneWay,
        false,
        false,
    ),
    prop(
        "fontSize",
        Some("font.size"),
        PropertyType::Number,
        InvalidationClass::Layout,
        BindingCapability::OneWay,
        true,
        true,
    ),
    prop(
        "fontFamily",
        Some("font.family"),
        PropertyType::Text,
        InvalidationClass::Layout,
        BindingCapability::OneWay,
        true,
        false,
    ),
    prop(
        "fontWeight",
        Some("font.weight"),
        PropertyType::Text,
        InvalidationClass::Layout,
        BindingCapability::OneWay,
        true,
        false,
    ),
    prop(
        "wrap",
        None,
        PropertyType::Boolean,
        InvalidationClass::Layout,
        BindingCapability::OneWay,
        true,
        false,
    ),
    prop(
        "value",
        None,
        PropertyType::Any,
        InvalidationClass::Paint,
        BindingCapability::TwoWayValue,
        false,
        false,
    ),
    prop(
        "checked",
        None,
        PropertyType::Boolean,
        InvalidationClass::Paint,
        BindingCapability::TwoWayChecked,
        false,
        false,
    ),
    prop(
        "selected",
        None,
        PropertyType::Boolean,
        InvalidationClass::Paint,
        BindingCapability::TwoWaySelected,
        false,
        false,
    ),
    prop(
        "group",
        None,
        PropertyType::Any,
        InvalidationClass::Paint,
        BindingCapability::TwoWayGroup,
        false,
        false,
    ),
    prop(
        "placeholder",
        None,
        PropertyType::Text,
        InvalidationClass::Paint,
        BindingCapability::OneWay,
        true,
        false,
    ),
    prop(
        "placeholderColor",
        None,
        PropertyType::Color,
        InvalidationClass::Paint,
        BindingCapability::OneWay,
        true,
        false,
    ),
    prop(
        "label",
        None,
        PropertyType::Text,
        InvalidationClass::Paint,
        BindingCapability::OneWay,
        false,
        false,
    ),
    prop(
        "multiline",
        None,
        PropertyType::Boolean,
        InvalidationClass::Layout,
        BindingCapability::OneWay,
        false,
        false,
    ),
    prop(
        "readOnly",
        None,
        PropertyType::Boolean,
        InvalidationClass::Paint,
        BindingCapability::OneWay,
        false,
        false,
    ),
    prop(
        "disabled",
        None,
        PropertyType::Boolean,
        InvalidationClass::Paint,
        BindingCapability::OneWay,
        true,
        true,
    ),
    prop(
        "color",
        None,
        PropertyType::Color,
        InvalidationClass::Paint,
        BindingCapability::OneWay,
        true,
        true,
    ),
    prop(
        "background",
        None,
        PropertyType::Color,
        InvalidationClass::Paint,
        BindingCapability::OneWay,
        true,
        true,
    ),
    prop(
        "hoverBackground",
        None,
        PropertyType::Color,
        InvalidationClass::Paint,
        BindingCapability::OneWay,
        true,
        true,
    ),
    prop(
        "pressedBackground",
        None,
        PropertyType::Color,
        InvalidationClass::Paint,
        BindingCapability::OneWay,
        true,
        true,
    ),
    prop(
        "disabledBackground",
        None,
        PropertyType::Color,
        InvalidationClass::Paint,
        BindingCapability::OneWay,
        true,
        true,
    ),
    prop(
        "hover",
        None,
        PropertyType::Color,
        InvalidationClass::Paint,
        BindingCapability::OneWay,
        true,
        false,
    ),
    prop(
        "pressed",
        None,
        PropertyType::Color,
        InvalidationClass::Paint,
        BindingCapability::OneWay,
        true,
        false,
    ),
    prop(
        "focus",
        None,
        PropertyType::Color,
        InvalidationClass::Paint,
        BindingCapability::OneWay,
        true,
        false,
    ),
    prop(
        "borderWidth",
        None,
        PropertyType::Number,
        InvalidationClass::Paint,
        BindingCapability::OneWay,
        true,
        true,
    ),
    prop(
        "borderColor",
        None,
        PropertyType::Color,
        InvalidationClass::Paint,
        BindingCapability::OneWay,
        true,
        true,
    ),
    prop(
        "focusBorderColor",
        None,
        PropertyType::Color,
        InvalidationClass::Paint,
        BindingCapability::OneWay,
        true,
        true,
    ),
    prop(
        "radius",
        None,
        PropertyType::Number,
        InvalidationClass::Paint,
        BindingCapability::OneWay,
        true,
        true,
    ),
    prop(
        "transition",
        None,
        PropertyType::Any,
        InvalidationClass::Paint,
        BindingCapability::OneWay,
        true,
        false,
    ),
    prop(
        "transitionDuration",
        None,
        PropertyType::Duration,
        InvalidationClass::Paint,
        BindingCapability::OneWay,
        true,
        true,
    ),
    prop(
        "clip",
        None,
        PropertyType::Boolean,
        InvalidationClass::Paint,
        BindingCapability::OneWay,
        true,
        false,
    ),
    prop(
        "overflow",
        None,
        PropertyType::Text,
        InvalidationClass::Paint,
        BindingCapability::OneWay,
        false,
        false,
    ),
    prop(
        "capture",
        None,
        PropertyType::Boolean,
        InvalidationClass::Structure,
        BindingCapability::OneWay,
        false,
        false,
    ),
    prop(
        "source",
        None,
        PropertyType::Asset,
        InvalidationClass::Paint,
        BindingCapability::OneWay,
        true,
        false,
    ),
    prop(
        "min",
        None,
        PropertyType::Number,
        InvalidationClass::Paint,
        BindingCapability::OneWay,
        false,
        false,
    ),
    prop(
        "max",
        None,
        PropertyType::Number,
        InvalidationClass::Paint,
        BindingCapability::OneWay,
        false,
        false,
    ),
    prop(
        "step",
        None,
        PropertyType::Number,
        InvalidationClass::Paint,
        BindingCapability::OneWay,
        false,
        false,
    ),
    prop(
        "backdrop",
        None,
        PropertyType::Color,
        InvalidationClass::Paint,
        BindingCapability::OneWay,
        true,
        false,
    ),
    prop(
        "dismiss",
        None,
        PropertyType::Boolean,
        InvalidationClass::Paint,
        BindingCapability::OneWay,
        false,
        false,
    ),
    prop(
        "open",
        None,
        PropertyType::Boolean,
        InvalidationClass::Structure,
        BindingCapability::OneWay,
        false,
        false,
    ),
    prop(
        "content",
        None,
        PropertyType::Any,
        InvalidationClass::Structure,
        BindingCapability::OneWay,
        false,
        false,
    ),
    prop(
        "slot",
        None,
        PropertyType::Text,
        InvalidationClass::Structure,
        BindingCapability::None,
        false,
        false,
    ),
    prop(
        "link",
        None,
        PropertyType::Text,
        InvalidationClass::Paint,
        BindingCapability::OneWay,
        false,
        false,
    ),
    prop(
        "targetX",
        None,
        PropertyType::Number,
        InvalidationClass::Paint,
        BindingCapability::OneWay,
        false,
        false,
    ),
    prop(
        "targetY",
        None,
        PropertyType::Number,
        InvalidationClass::Paint,
        BindingCapability::OneWay,
        false,
        false,
    ),
    prop(
        "targetWidth",
        None,
        PropertyType::Number,
        InvalidationClass::Paint,
        BindingCapability::OneWay,
        false,
        false,
    ),
    prop(
        "targetHeight",
        None,
        PropertyType::Number,
        InvalidationClass::Paint,
        BindingCapability::OneWay,
        false,
        false,
    ),
    prop(
        "targetRadius",
        None,
        PropertyType::Number,
        InvalidationClass::Paint,
        BindingCapability::OneWay,
        false,
        false,
    ),
    prop(
        "activeRadius",
        None,
        PropertyType::Number,
        InvalidationClass::Paint,
        BindingCapability::OneWay,
        false,
        false,
    ),
    prop(
        "stop",
        None,
        PropertyType::Color,
        InvalidationClass::Paint,
        BindingCapability::OneWay,
        false,
        false,
    ),
    prop(
        "activeStop",
        None,
        PropertyType::Color,
        InvalidationClass::Paint,
        BindingCapability::OneWay,
        false,
        false,
    ),
];

/// Finds the canonical registry entry for a property name or its alias.
pub fn lookup_property(name: &str) -> Option<&'static PropertySpec> {
    PROPERTIES
        .iter()
        .find(|spec| spec.name == name || spec.alias == Some(name))
}

/// Normalizes a dotted alias (such as `font.size`) to its canonical spelling (`fontSize`).
pub fn canonical_property_name(name: &str) -> &str {
    lookup_property(name).map_or(name, |spec| spec.name)
}

/// Reports whether a property accepts `#RGB` / `#RRGGBB` / `#RRGGBBAA` hex colors.
pub fn is_color_property(name: &str) -> bool {
    if let Some(spec) = lookup_property(name) {
        return spec.ty == PropertyType::Color;
    }
    name.as_bytes()
        .windows(5)
        .any(|w| w.eq_ignore_ascii_case(b"color"))
}

/// Reports whether a property is retained directly on the rendered root control node.
pub fn is_scene_control_property(name: &str) -> bool {
    lookup_property(name).is_some_and(|spec| spec.scene_control && spec.name == name)
}

const FRAME_PROPS: &[&str] = &[
    "key",
    "x",
    "y",
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
    "background",
    "radius",
    "clip",
    "overflow",
    "capture",
    "cell",
    "row",
    "column",
    "row.span",
    "column.span",
    "transition",
];
const LINEAR_CONTAINER_PROPS: &[&str] = &[
    "key",
    "x",
    "y",
    "width",
    "height",
    "minWidth",
    "maxWidth",
    "minHeight",
    "maxHeight",
    "padding",
    "gap",
    "clip",
    "radius",
    "capture",
    "cell",
    "row",
    "column",
    "row.span",
    "column.span",
    "transition",
];
const SCROLL_PROPS: &[&str] = &[
    "key",
    "x",
    "y",
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
    "clip",
    "cell",
    "row",
    "column",
    "row.span",
    "column.span",
];
const MODAL_PROPS: &[&str] = &[
    "key", "x", "y", "width", "height", "backdrop", "dismiss", "open",
];
const RECTANGLE_PROPS: &[&str] = &[
    "key",
    "cell",
    "row",
    "column",
    "row.span",
    "column.span",
    "width",
    "height",
    "minWidth",
    "maxWidth",
    "minHeight",
    "maxHeight",
    "background",
    "radius",
    "transition",
];
const TEXT_PROPS: &[&str] = &[
    "key",
    "cell",
    "row",
    "column",
    "row.span",
    "column.span",
    "width",
    "height",
    "minWidth",
    "maxWidth",
    "minHeight",
    "maxHeight",
    "text",
    "runs",
    "color",
    "fontSize",
    "font.size",
    "fontFamily",
    "font.family",
    "fontWeight",
    "font.weight",
    "wrap",
    "transition",
];
const TEXT_INPUT_PROPS: &[&str] = &[
    "key",
    "cell",
    "row",
    "column",
    "row.span",
    "column.span",
    "width",
    "height",
    "minWidth",
    "maxWidth",
    "minHeight",
    "maxHeight",
    "value",
    "placeholder",
    "label",
    "color",
    "placeholderColor",
    "fontSize",
    "font.size",
    "multiline",
    "readOnly",
    "wrap",
    "disabled",
    "transition",
];
const IMAGE_PROPS: &[&str] = &[
    "key",
    "cell",
    "row",
    "column",
    "row.span",
    "column.span",
    "width",
    "height",
    "minWidth",
    "maxWidth",
    "minHeight",
    "maxHeight",
    "source",
    "color",
    "transition",
];
const CANVAS_PROPS: &[&str] = &[
    "key",
    "cell",
    "row",
    "column",
    "row.span",
    "column.span",
    "x",
    "y",
    "width",
    "height",
    "minWidth",
    "maxWidth",
    "minHeight",
    "maxHeight",
];
const PRESENTER_PROPS: &[&str] = &["key", "slot", "content"];

/// Canonical property names recognized directly by each built-in element kind.
pub fn allowed_properties_for(kind: BuiltinKind) -> &'static [&'static str] {
    match kind {
        BuiltinKind::Frame | BuiltinKind::Grid => FRAME_PROPS,
        BuiltinKind::Row | BuiltinKind::Column | BuiltinKind::Stack => LINEAR_CONTAINER_PROPS,
        BuiltinKind::Scroll => SCROLL_PROPS,
        BuiltinKind::Modal => MODAL_PROPS,
        BuiltinKind::Rectangle => RECTANGLE_PROPS,
        BuiltinKind::Text => TEXT_PROPS,
        BuiltinKind::TextInput => TEXT_INPUT_PROPS,
        BuiltinKind::Image => IMAGE_PROPS,
        BuiltinKind::Canvas => CANVAS_PROPS,
        BuiltinKind::ContentPresenter => PRESENTER_PROPS,
        _ => &[],
    }
}

/// Records root state keys referenced in a `Value` tree (`state.foo.bar` -> `"foo"`,
/// plus leading identifiers in case they resolve against scoped/model state).
pub fn collect_value_dependencies(value: &Value, state_roots: &mut BTreeSet<String>) {
    collect_prefixed_dependencies(value, false, state_roots);
}

fn collect_prefixed_dependencies(value: &Value, props_mode: bool, out: &mut BTreeSet<String>) {
    match value {
        Value::Expr(path) => {
            let trimmed = path.strip_prefix('!').unwrap_or(path);
            if props_mode {
                if let Some(tail) = trimmed.strip_prefix("props.") {
                    if let Some(root) = tail.split('.').next().filter(|s| !s.is_empty()) {
                        out.insert(root.to_owned());
                    }
                }
            } else if let Some(tail) = trimmed.strip_prefix("state.") {
                if let Some(root) = tail.split('.').next().filter(|s| !s.is_empty()) {
                    out.insert(root.to_owned());
                }
            } else if !trimmed.starts_with("props.")
                && !trimmed.starts_with("tokens.")
                && !trimmed.starts_with("i18n.")
                && trimmed != "null"
                && trimmed != "_"
            {
                if let Some(root) = trimmed.split('.').next().filter(|s| {
                    s.chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                }) {
                    out.insert(root.to_owned());
                }
            }
        }
        Value::Expression(expr) => match expr.as_ref() {
            super::Expression::Unary(_, arg) => collect_prefixed_dependencies(arg, props_mode, out),
            super::Expression::Binary(_, left, right) => {
                collect_prefixed_dependencies(left, props_mode, out);
                collect_prefixed_dependencies(right, props_mode, out);
            }
            super::Expression::Conditional(c, t, e) => {
                collect_prefixed_dependencies(c, props_mode, out);
                collect_prefixed_dependencies(t, props_mode, out);
                collect_prefixed_dependencies(e, props_mode, out);
            }
            super::Expression::Member(obj, prop, _) => {
                collect_prefixed_dependencies(obj, props_mode, out);
                collect_prefixed_dependencies(prop, props_mode, out);
            }
            super::Expression::Call(_, args) => {
                for arg in args {
                    collect_prefixed_dependencies(arg, props_mode, out);
                }
            }
        },
        Value::Interpolation(parts) | Value::List(parts) => {
            for part in parts {
                collect_prefixed_dependencies(part, props_mode, out);
            }
        }
        Value::Object(obj) => {
            for val in obj.values() {
                collect_prefixed_dependencies(val, props_mode, out);
            }
        }
        Value::Match(subject, branches) => {
            collect_prefixed_dependencies(subject, props_mode, out);
            for (pat, val) in branches {
                collect_prefixed_dependencies(pat, props_mode, out);
                collect_prefixed_dependencies(val, props_mode, out);
            }
        }
        Value::Node(node) => {
            for val in node.props.values() {
                collect_prefixed_dependencies(val, props_mode, out);
            }
            for child in &node.children {
                for val in child.props.values() {
                    collect_prefixed_dependencies(val, props_mode, out);
                }
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Text(_) => {}
    }
}

/// Summarizes which invalidation class each root state key triggers across a `Document`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DocumentDependencySummary {
    pub state_classes: BTreeMap<String, InvalidationClass>,
}

impl DocumentDependencySummary {
    fn record(&mut self, root: String, class: InvalidationClass) {
        self.state_classes
            .entry(root)
            .and_modify(|existing| *existing = existing.combine(class))
            .or_insert(class);
    }

    /// Computes the highest invalidation class touched when `states[0]` changes from `prev` to `next`.
    pub fn classify_state_change(&self, prev: &Properties, next: &Properties) -> InvalidationClass {
        let mut class = InvalidationClass::None;
        for (key, value) in next.iter() {
            if prev.get(key) != Some(value) {
                let entry = self
                    .state_classes
                    .get(key.as_str())
                    .copied()
                    .unwrap_or(InvalidationClass::Paint);
                class = class.combine(entry);
            }
        }
        for key in prev.keys() {
            if !next.contains_key(key.as_str()) {
                let entry = self
                    .state_classes
                    .get(key.as_str())
                    .copied()
                    .unwrap_or(InvalidationClass::Paint);
                class = class.combine(entry);
            }
        }
        class
    }
}

fn collect_def_prop_classes(
    node: &Node,
    def_prop_map: &BTreeMap<String, BTreeMap<String, InvalidationClass>>,
    prop_classes: &mut BTreeMap<String, InvalidationClass>,
) {
    let mut record_prop = |name: String, class: InvalidationClass| {
        prop_classes
            .entry(name)
            .and_modify(|existing| *existing = existing.combine(class))
            .or_insert(class);
    };

    if matches!(
        node.kind.builtin(),
        Some(BuiltinKind::If | BuiltinKind::For | BuiltinKind::Match | BuiltinKind::Branch)
    ) {
        let mut names = BTreeSet::new();
        for key in [
            "condition",
            "collection",
            "subject",
            "pattern",
            "guard",
            "key",
        ] {
            if let Some(val) = node.props.get(key) {
                collect_prefixed_dependencies(val, true, &mut names);
            }
        }
        for name in names {
            record_prop(name, InvalidationClass::Structure);
        }
    }

    let child_def_props = def_prop_map.get(node.kind.as_str());
    for (key, val) in &node.props {
        if matches!(
            key.as_str(),
            "condition" | "collection" | "subject" | "pattern" | "guard" | "__node" | "__state"
        ) {
            continue;
        }
        let class = match key.as_str() {
            "__matches" => InvalidationClass::Layout,
            _ => child_def_props
                .and_then(|m| m.get(key.as_str()).copied())
                .or_else(|| lookup_property(key).map(|spec| spec.invalidation))
                .unwrap_or(InvalidationClass::Paint),
        };
        let mut names = BTreeSet::new();
        collect_prefixed_dependencies(val, true, &mut names);
        for name in names {
            record_prop(name, class);
        }
    }
    for child in &node.children {
        collect_def_prop_classes(child, def_prop_map, prop_classes);
    }
}

fn scan_node(
    node: &Node,
    def_prop_map: &BTreeMap<String, BTreeMap<String, InvalidationClass>>,
    prop_links: InvalidationClass,
    summary: &mut DocumentDependencySummary,
) {
    match node.kind.builtin() {
        Some(BuiltinKind::If | BuiltinKind::For | BuiltinKind::Match | BuiltinKind::Branch) => {
            let mut roots = BTreeSet::new();
            for key in [
                "condition",
                "collection",
                "subject",
                "pattern",
                "guard",
                "key",
            ] {
                if let Some(val) = node.props.get(key) {
                    collect_value_dependencies(val, &mut roots);
                }
            }
            for root in roots {
                summary.record(root, InvalidationClass::Structure);
            }
        }
        Some(BuiltinKind::Let) => {
            let mut roots = BTreeSet::new();
            if let Some(val) = node.props.get("value") {
                collect_value_dependencies(val, &mut roots);
            }
            for root in roots {
                summary.record(root, prop_links.combine(InvalidationClass::Paint));
            }
        }
        _ => {}
    }

    let child_def_props = def_prop_map.get(node.kind.as_str());
    for (key, val) in &node.props {
        if matches!(
            key.as_str(),
            "condition" | "collection" | "subject" | "pattern" | "guard" | "__node" | "__state"
        ) {
            continue;
        }
        let class = match key.as_str() {
            "__matches" => InvalidationClass::Layout,
            _ => child_def_props
                .and_then(|m| m.get(key.as_str()).copied())
                .or_else(|| lookup_property(key).map(|spec| spec.invalidation))
                .unwrap_or(prop_links),
        };
        let mut roots = BTreeSet::new();
        collect_value_dependencies(val, &mut roots);
        for root in roots {
            summary.record(root, class);
        }
    }

    for child in &node.children {
        scan_node(child, def_prop_map, prop_links, summary);
    }
}

/// Analyzes `doc.root` and all referenced component definitions into a `DocumentDependencySummary`.
pub fn analyze_document_dependencies(doc: &Document) -> DocumentDependencySummary {
    let mut def_prop_map: BTreeMap<String, BTreeMap<String, InvalidationClass>> = BTreeMap::new();
    // Propagate nested component prop invalidation classes across definition depth (bounded by tree depth).
    for _ in 0..8 {
        let mut changed = false;
        for (name, def) in &doc.definitions {
            let mut classes = def_prop_map.get(name).cloned().unwrap_or_default();
            for node in &def.nodes {
                collect_def_prop_classes(node, &def_prop_map, &mut classes);
            }
            if def_prop_map.get(name) != Some(&classes) {
                def_prop_map.insert(name.clone(), classes);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let mut summary = DocumentDependencySummary::default();
    scan_node(
        &doc.root,
        &def_prop_map,
        InvalidationClass::Paint,
        &mut summary,
    );
    for def in doc.definitions.values() {
        for node in &def.nodes {
            scan_node(node, &def_prop_map, InvalidationClass::Paint, &mut summary);
        }
    }
    summary
}

/// Serializes the full `.ui` property registry and per-element allowlist as JSON.
pub fn property_registry_json() -> String {
    #[derive(serde::Serialize)]
    struct Wire<'a> {
        properties: &'static [PropertySpec],
        nodes: BTreeMap<&'a str, &'static [&'static str]>,
    }

    let mut nodes = BTreeMap::new();
    for kind in [
        BuiltinKind::Frame,
        BuiltinKind::Row,
        BuiltinKind::Column,
        BuiltinKind::Grid,
        BuiltinKind::Stack,
        BuiltinKind::Scroll,
        BuiltinKind::Modal,
        BuiltinKind::Rectangle,
        BuiltinKind::Text,
        BuiltinKind::TextInput,
        BuiltinKind::Image,
        BuiltinKind::Canvas,
        BuiltinKind::ContentPresenter,
    ] {
        nodes.insert(kind.as_str(), allowed_properties_for(kind));
    }

    serde_json::to_string(&Wire {
        properties: PROPERTIES,
        nodes,
    })
    .unwrap_or_else(|_| "{}".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_covers_types_aliases_bindings_and_styleability() {
        let font_alias = lookup_property("font.size").unwrap();
        assert_eq!(font_alias.name, "fontSize");
        assert_eq!(font_alias.invalidation, InvalidationClass::Layout);
        assert!(font_alias.styleable);
        assert!(font_alias.scene_control);
        assert!(is_scene_control_property("fontSize"));
        assert!(!is_scene_control_property("font.size"));

        assert_eq!(
            lookup_property("value").unwrap().binding,
            BindingCapability::TwoWayValue
        );
        assert_eq!(
            lookup_property("checked").unwrap().binding,
            BindingCapability::TwoWayChecked
        );
        assert_eq!(
            lookup_property("selected").unwrap().binding,
            BindingCapability::TwoWaySelected
        );
        assert_eq!(
            lookup_property("group").unwrap().binding,
            BindingCapability::TwoWayGroup
        );
        assert!(!lookup_property("columns").unwrap().styleable);
        assert!(is_color_property("focusBorderColor"));
        assert!(is_color_property("customAccentColor"));
        assert!(!is_color_property("borderWidth"));
        assert!(allowed_properties_for(BuiltinKind::Text).contains(&"fontWeight"));
        assert!(allowed_properties_for(BuiltinKind::TextInput).contains(&"readOnly"));

        let json: serde_json::Value = serde_json::from_str(&property_registry_json()).unwrap();
        assert!(json["properties"].as_array().unwrap().len() >= PROPERTIES.len());
        assert!(
            json["nodes"]["Text"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v == "runs")
        );
    }

    #[test]
    fn document_dependency_summary_distinguishes_structure_layout_and_paint_fields() {
        let files = BTreeMap::from([
            (
                "components/BadgeCard.ui".into(),
                "component BadgeCard { headingSize: 16; title: ''; width: 120; height: 40; Rectangle { width: 120; height: 40; background: #1e293b; Text { text: props.title; fontSize: props.headingSize; } } }".into(),
            ),
            (
                "App.ui".into(),
                "component App { state show: Bool = true; state title: String = 'Hi'; state w: Number = 120; state heading: Number = 18; Column { width: 300; height: 200; if state.show { BadgeCard { key: 'b'; width: state.w; height: 40; title: state.title; headingSize: state.heading; } } } }".into(),
            ),
        ]);
        let doc = Document::from_ui_files(&files, "App.ui", &Properties::new()).unwrap();
        let summary = analyze_document_dependencies(&doc);
        assert_eq!(
            summary.state_classes.get("show").copied(),
            Some(InvalidationClass::Structure)
        );
        assert_eq!(
            summary.state_classes.get("w").copied(),
            Some(InvalidationClass::Layout)
        );
        assert_eq!(
            summary.state_classes.get("heading").copied(),
            Some(InvalidationClass::Layout)
        );
        assert_eq!(
            summary.state_classes.get("title").copied(),
            Some(InvalidationClass::Paint)
        );

        let base = doc.states[0].clone();
        let mut paint_only = base.clone();
        paint_only.insert("title".into(), Value::Text("Updated".into()));
        assert_eq!(
            summary.classify_state_change(&base, &paint_only),
            InvalidationClass::Paint
        );

        let mut layout_change = paint_only.clone();
        layout_change.insert("w".into(), Value::Number(160.));
        assert_eq!(
            summary.classify_state_change(&base, &layout_change),
            InvalidationClass::Layout
        );
    }
}
