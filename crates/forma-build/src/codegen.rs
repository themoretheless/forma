//! Generates typed Rust form wrappers and embedded `Document` definitions from parsed `.ui` ASTs.

use crate::expr::expression_references;
use crate::linker::{fallback_defaults, link_component_definitions, parse_ui_document};
use crate::svg::parse_svg_image;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

const LAYOUT_TYPES: &[&str] = &["Frame", "Scroll", "Row", "Column", "Grid", "Stack"];

const STANDARD_CONTROL_PROPS: &[&str] = &[
    "x",
    "y",
    "width",
    "height",
    "text",
    "fontSize",
    "font.size",
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
    "cell",
    "row",
    "column",
    "minWidth",
    "maxWidth",
    "minHeight",
    "maxHeight",
];

const LAYOUT_ALLOWED_PROPS: &[&str] = &[
    "x",
    "y",
    "width",
    "height",
    "padding",
    "gap",
    "background",
    "columns",
    "rows",
    "clip",
    "radius",
    "overflow",
    "layout",
    "cell",
    "row",
    "column",
    "minWidth",
    "maxWidth",
    "minHeight",
    "maxHeight",
    "align",
    "justify",
];

const RUST_RESERVED: &[&str] = &[
    "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn", "for",
    "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return",
    "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where",
    "while", "async", "await", "dyn", "abstract", "become", "box", "do", "final", "macro",
    "override", "priv", "typeof", "unsized", "virtual", "yield", "try", "gen",
];

const BUILTIN_METHODS: &[&str] = &[
    "new",
    "with_properties",
    "context",
    "set_context",
    "clear_context",
    "sync",
    "update",
    "runtime",
    "is_dirty",
    "on_dirty",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceLocation {
    pub file: String,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedForm {
    pub rust: String,
    pub source_map: BTreeMap<usize, SourceLocation>,
    pub form_name: String,
}

pub(crate) fn is_valid_ident(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn is_valid_rust_path(path: &str) -> bool {
    !path.is_empty() && path.split("::").all(is_valid_ident)
}

pub(crate) fn snake_case(name: &str) -> String {
    let mut step1 = String::with_capacity(name.len() + 4);
    let chars: Vec<char> = name.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_uppercase() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_uppercase() {
                i += 1;
            }
            if i < chars.len() && chars[i].is_ascii_lowercase() && i - start >= 2 {
                let split = i - 1;
                for ch in &chars[start..split] {
                    step1.push(*ch);
                }
                step1.push('_');
                step1.push(chars[split]);
                step1.push(chars[i]);
                i += 1;
            } else {
                for ch in &chars[start..i] {
                    step1.push(*ch);
                }
            }
        } else {
            step1.push(chars[i]);
            i += 1;
        }
    }
    let mut step2 = String::with_capacity(step1.len() + 4);
    let chars2: Vec<char> = step1.chars().collect();
    for idx in 0..chars2.len() {
        let ch = chars2[idx];
        step2.push(ch);
        if (ch.is_ascii_lowercase() || ch.is_ascii_digit())
            && chars2.get(idx + 1).is_some_and(char::is_ascii_uppercase)
        {
            step2.push('_');
        }
    }
    step2.to_ascii_lowercase()
}

pub(crate) fn rust_string(value: &str) -> String {
    let mut hashes = String::from("#");
    while value.contains(&format!("\"{hashes}")) {
        hashes.push('#');
    }
    format!("r{hashes}\"{value}\"{hashes}")
}

fn format_f64(value: f64) -> String {
    let clean = if value == 0.0 { 0.0 } else { value };
    format!("{clean:.12}")
}

fn line_at_utf16_offset(source: &str, offset_utf16: usize) -> usize {
    let mut line = 1usize;
    let mut utf16 = 0usize;
    for ch in source.chars() {
        if utf16 >= offset_utf16 {
            break;
        }
        if ch == '\n' {
            line += 1;
        }
        utf16 += ch.len_utf16();
    }
    line
}

fn offset_from_range(val: Option<&Value>) -> Option<usize> {
    val.and_then(|v| v.get("from"))
        .and_then(Value::as_u64)
        .map(|n| n as usize)
}

fn lookup_state_path<'a>(state: &'a Value, dot_path: &str) -> Option<&'a Value> {
    let mut cur = state;
    for part in dot_path.split('.') {
        cur = cur.as_object()?.get(part)?;
    }
    if cur.is_null() && state.is_null() {
        None
    } else {
        Some(cur)
    }
}

fn svg_entry_rust(name: &str, source: &str) -> Result<String, String> {
    let image = parse_svg_image(source)?;
    let shapes = image
        .shapes
        .iter()
        .map(|shape| {
            let pts = shape
                .points
                .iter()
                .map(|[x, y]| format!("[{},{}]", format_f64(*x), format_f64(*y)))
                .collect::<Vec<_>>()
                .join(",");
            format!("(vec![{pts}],{}.into())", rust_string(&shape.color))
        })
        .collect::<Vec<_>>()
        .join(",");
    Ok(format!(
        "({}.into(), ::forma::form_document::Image {{width:{},height:{},shapes:vec![{shapes}]}})",
        rust_string(name),
        format_f64(image.width),
        format_f64(image.height),
    ))
}

#[derive(Clone)]
struct LoopScope {
    owner: String,
    field: String,
    indices: Vec<String>,
    access: Vec<String>,
}

struct EventDescriptor {
    context_type: Option<String>,
    controls: Vec<(String, String)>,
    args: Vec<EventArg>,
}

struct EventArg {
    name: String,
    ty: String,
}

struct Generator<'a> {
    entry: &'a str,
    source: &'a str,
    state: &'a Value,
    linked: &'a BTreeMap<String, Value>,
    fallback: &'a Map<String, Value>,
    form_name: String,
    statements: Vec<String>,
    fields: Vec<String>,
    methods: Vec<String>,
    controls: Vec<String>,
    event_structs: Vec<String>,
    used_names: BTreeSet<String>,
    used_definitions: BTreeSet<String>,
    event_types: BTreeMap<String, EventDescriptor>,
    event_order: Vec<String>,
    collection_blueprints: BTreeMap<String, Value>,
    serial: usize,
    node_serial: usize,
    control_count: usize,
}

impl<'a> Generator<'a> {
    fn fail(&self, message: impl std::fmt::Display, offset: usize) -> String {
        let line = line_at_utf16_offset(self.source, offset);
        format!("{}:{line}: {message}", self.entry)
    }

    fn valid_name(&self, name: &str, offset: usize) -> Result<String, String> {
        if !is_valid_ident(name) || RUST_RESERVED.contains(&name) {
            return Err(self.fail(format!("Недопустимое Rust-имя «{name}»"), offset));
        }
        Ok(name.to_string())
    }

    fn valid_type_path(&self, raw: Option<&str>, offset: usize) -> Result<String, String> {
        match raw {
            Some(path) if is_valid_rust_path(path) => Ok(path.to_string()),
            _ => Err(self.fail(
                "contextType ожидает Rust-тип в строке, например 'crate::CustomerVm'",
                offset,
            )),
        }
    }

    fn parse_state_path(&self, expr: Option<&str>, offset: usize) -> Result<Vec<String>, String> {
        let Some(expr) = expr.and_then(|s| s.strip_prefix("state.")) else {
            return Err(self.fail("Ожидается путь state.field", offset));
        };
        expr.split('.')
            .map(|seg| self.valid_name(seg, offset))
            .collect()
    }

    fn project(&mut self, parent: &str, segments: &[String]) -> String {
        let mut current = parent.to_string();
        for field in segments {
            let next = format!("scope_{}", self.serial);
            self.serial += 1;
            self.statements.push(format!(
                "let {next} = ::forma::binding::Context::project(&{current}, |vm| vm.{field}.clone());"
            ));
            current = next;
        }
        current
    }

    fn scope(
        &mut self,
        node: &Value,
        parent: &str,
        parent_type: &str,
    ) -> Result<(String, String), String> {
        let props = node.get("props").and_then(Value::as_object);
        let has_context = props.is_some_and(|p| p.contains_key("context"));
        let has_context_type = props.is_some_and(|p| p.contains_key("contextType"));
        let start = node.get("start").and_then(Value::as_u64).unwrap_or(0) as usize;
        if has_context {
            let ctx_offset =
                offset_from_range(node.get("statementRanges").and_then(|r| r.get("context")))
                    .unwrap_or(start);
            let ctx_type_raw = props
                .and_then(|p| p.get("contextType"))
                .and_then(Value::as_str);
            let ty = self.valid_type_path(ctx_type_raw, ctx_offset)?;
            let ctx_expr = props
                .and_then(|p| p.get("context"))
                .and_then(|c| c.get("expr"))
                .and_then(Value::as_str);
            let segments = self.parse_state_path(ctx_expr, ctx_offset)?;
            let current = self.project(parent, &segments);
            self.statements.push(format!(
                "let {current}: ::forma::binding::Context<{ty}> = {current};"
            ));
            Ok((current, ty))
        } else {
            if has_context_type {
                return Err(self.fail("contextType у узла требует context", start));
            }
            let current = format!("scope_{}", self.serial);
            self.serial += 1;
            self.statements.push(format!(
                "let {current} = ::forma::binding::Context::child(&{parent});"
            ));
            Ok((current, parent_type.to_string()))
        }
    }

    fn collection_projection(&self, loop_node: &Value, collection: &str) -> Result<String, String> {
        let item = loop_node
            .get("item")
            .and_then(Value::as_str)
            .unwrap_or("item");
        let start = loop_node.get("start").and_then(Value::as_u64).unwrap_or(0) as usize;
        let item_prefix = format!("{item}.");
        let scope_val = json!({
            "key": loop_node.get("key").unwrap_or(&Value::Null),
            "children": loop_node.get("children").unwrap_or(&Value::Null),
        });
        let mut references: Vec<String> = expression_references(&scope_val)
            .into_iter()
            .filter(|r| r.starts_with(&item_prefix))
            .collect();
        let mut nested: BTreeMap<String, Value> = BTreeMap::new();
        let mut bare = false;

        fn scan(
            value: &Value,
            loop_node: &Value,
            item: &str,
            item_prefix: &str,
            bare: &mut bool,
            nested: &mut BTreeMap<String, Value>,
            references: &mut Vec<String>,
        ) {
            match value {
                Value::Array(items) => {
                    for child in items {
                        scan(
                            child,
                            loop_node,
                            item,
                            item_prefix,
                            bare,
                            nested,
                            references,
                        );
                    }
                }
                Value::Object(map) => {
                    if map.get("expr").and_then(Value::as_str) == Some(item) {
                        *bare = true;
                    }
                    if map.get("type").and_then(Value::as_str) == Some("For")
                        && value != loop_node
                        && let Some(coll_expr) = map
                            .get("collection")
                            .and_then(|c| c.get("expr"))
                            .and_then(Value::as_str)
                        && let Some(tail) = coll_expr.strip_prefix(item_prefix)
                    {
                        nested.insert(tail.to_string(), value.clone());
                    }
                    if let Some(bindings) = map.get("bindings").and_then(Value::as_object) {
                        for expr_val in bindings.values() {
                            if let Some(expr_str) = expr_val.as_str()
                                && expr_str.starts_with(item_prefix)
                            {
                                references.push(expr_str.to_string());
                            }
                        }
                    }
                    for child in map.values() {
                        scan(
                            child,
                            loop_node,
                            item,
                            item_prefix,
                            bare,
                            nested,
                            references,
                        );
                    }
                }
                _ => {}
            }
        }
        scan(
            &scope_val,
            loop_node,
            item,
            &item_prefix,
            &mut bare,
            &mut nested,
            &mut references,
        );

        let build_item = if bare {
            format!("::forma::binding::ModelValue::to_value({item})")
        } else {
            let paths: Vec<Vec<String>> = references
                .iter()
                .map(|r| {
                    r[item_prefix.len()..]
                        .split('.')
                        .map(str::to_string)
                        .collect()
                })
                .collect();
            self.build_projection(item, &paths, "", &nested, start)?
        };
        Ok(format!(
            "::forma::form_document::Value::List({collection}.iter().map(|{item}| {build_item}).collect())"
        ))
    }

    fn build_projection(
        &self,
        root: &str,
        paths: &[Vec<String>],
        prefix: &str,
        nested: &BTreeMap<String, Value>,
        start: usize,
    ) -> Result<String, String> {
        let mut groups: BTreeMap<String, Vec<Vec<String>>> = BTreeMap::new();
        for path in paths {
            let Some((head, tail)) = path.split_first() else {
                continue;
            };
            groups.entry(head.clone()).or_default().push(tail.to_vec());
        }
        let mut entries = Vec::with_capacity(groups.len());
        for (field, tails) in groups {
            self.valid_name(&field, start)?;
            let part = format!("{prefix}{field}");
            let value = if let Some(child_loop) = nested.get(&part) {
                self.collection_projection(child_loop, &format!("{root}.{field}"))?
            } else if tails.iter().any(Vec::is_empty) {
                format!("::forma::binding::ModelValue::to_value(&{root}.{field})")
            } else {
                self.build_projection(
                    &format!("{root}.{field}"),
                    &tails,
                    &format!("{part}."),
                    nested,
                    start,
                )?
            };
            entries.push(format!("({},{value})", rust_string(&field)));
        }
        Ok(format!(
            "::forma::form_document::Value::Object(::forma::form_document::props(vec![{}]))",
            entries.join(",")
        ))
    }

    fn collect_definition(&mut self, name: &str, seen: &mut BTreeSet<String>) {
        if !seen.insert(name.to_string()) {
            return;
        }
        let Some(def) = self.linked.get(name) else {
            return;
        };
        self.used_definitions.insert(name.to_string());
        fn nested_types(value: &Value, out: &mut Vec<String>) {
            match value {
                Value::Array(arr) => {
                    for item in arr {
                        nested_types(item, out);
                    }
                }
                Value::Object(map) => {
                    if let Some(ty) = map.get("type").and_then(Value::as_str) {
                        out.push(ty.to_string());
                    }
                    for child in map.values() {
                        nested_types(child, out);
                    }
                }
                _ => {}
            }
        }
        let mut child_types = Vec::new();
        nested_types(def, &mut child_types);
        for ty in child_types {
            self.collect_definition(&ty, seen);
        }
    }

    fn walk(
        &mut self,
        node: &mut Value,
        doc: &Value,
        parent: &str,
        parent_type: &str,
        loops: &BTreeMap<String, LoopScope>,
    ) -> Result<(), String> {
        let start = node.get("start").and_then(Value::as_u64).unwrap_or(0) as usize;
        if node
            .get("slots")
            .and_then(Value::as_object)
            .is_some_and(|s| !s.is_empty())
        {
            return Err(self.fail("override объявляется в наследнике component", start));
        }
        let node_index = self.node_serial;
        self.node_serial += 1;
        if let Some(obj) = node.as_object_mut() {
            obj.insert("__form_index".into(), Value::from(node_index));
        }
        let (current, ty) = self.scope(node, parent, parent_type)?;
        let node_type = node
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();

        if node_type == "If" || node_type == "For" {
            let mut next_loops = loops.clone();
            let item_name = node
                .get("item")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            if node_type == "For" && loops.contains_key(&item_name) {
                return Err(self.fail(
                    format!("Имя вложенного for перекрывает внешнее: {item_name}"),
                    start,
                ));
            }
            let target_expr_val = if node_type == "If" {
                node.get("condition").unwrap_or(&Value::Null)
            } else {
                node.get("collection").unwrap_or(&Value::Null)
            };
            let coll_expr_str = node
                .get("collection")
                .and_then(|c| c.get("expr"))
                .and_then(Value::as_str)
                .map(str::to_string);
            let deps: Vec<String> = expression_references(target_expr_val)
                .into_iter()
                .filter(|p| p.starts_with("state."))
                .collect();
            for expr in deps {
                let mut segments = self.parse_state_path(Some(&expr), start)?;
                let field = segments.pop().expect("non-empty path");
                let owner = self.project(&current, &segments);
                if node_type == "For" && coll_expr_str.as_deref() == Some(expr.as_str()) {
                    let projection = self.collection_projection(node, "items")?;
                    self.statements.push(format!(
                        "inner.observe_projected({node_index}, {}, &{owner}, |vm| vm.{field}.clone(), |items| {projection});",
                        rust_string(&expr[6..])
                    ));
                    next_loops.insert(
                        item_name.clone(),
                        LoopScope {
                            owner,
                            field,
                            indices: vec![format!("__index.{item_name}")],
                            access: Vec::new(),
                        },
                    );
                } else {
                    self.statements.push(format!(
                        "inner.observe({node_index}, {}, &{owner}, |vm| vm.{field}.clone());",
                        rust_string(&expr[6..])
                    ));
                }
            }
            if node_type == "For" && !next_loops.contains_key(&item_name) {
                let segments: Vec<&str> = coll_expr_str
                    .as_deref()
                    .map(|s| s.split('.').collect())
                    .unwrap_or_default();
                let parent_loop = segments.first().and_then(|root| next_loops.get(*root));
                let Some(parent_loop) = parent_loop.filter(|_| segments.len() >= 2) else {
                    return Err(self.fail(
                        "Коллекция for требует путь state.collection или поле внешнего элемента",
                        start,
                    ));
                };
                let access = segments[1..]
                    .iter()
                    .map(|seg| self.valid_name(seg, start))
                    .collect::<Result<Vec<_>, _>>()?
                    .join(".");
                let mut cloned = parent_loop.clone();
                cloned.indices.push(format!("__index.{item_name}"));
                cloned.access.push(access);
                next_loops.insert(item_name, cloned);
            }

            if let Some(children) = node.get_mut("children").and_then(Value::as_array_mut) {
                for child in children {
                    self.walk(child, doc, &current, &ty, &next_loops)?;
                }
            }
            for branch_key in ["elseChildren", "emptyChildren"] {
                if let Some(children) = node.get_mut(branch_key).and_then(Value::as_array_mut) {
                    for child in children {
                        self.walk(child, doc, &current, &ty, &next_loops)?;
                    }
                }
            }
            return Ok(());
        }

        let definition = self.linked.get(&node_type).cloned();
        let is_layout = LAYOUT_TYPES.contains(&node_type.as_str());
        if definition.is_none() && !is_layout {
            return Err(self.fail(format!("Неизвестный компонент {node_type}"), start));
        }

        let mut allowed: BTreeSet<String> =
            ["key", "context", "contextType", "row.span", "column.span"]
                .into_iter()
                .map(str::to_string)
                .collect();
        for prop in if is_layout {
            LAYOUT_ALLOWED_PROPS
        } else {
            STANDARD_CONTROL_PROPS
        } {
            allowed.insert((*prop).to_string());
        }
        if let Some(ref def) = definition {
            if let Some(defaults) = def.get("defaults").and_then(Value::as_object) {
                allowed.extend(defaults.keys().cloned());
            }
            if let Some(prop_defs) = def.get("propDefinitions").and_then(Value::as_object) {
                allowed.extend(prop_defs.keys().cloned());
            }
            if let Some(matches) = def.get("matches").and_then(Value::as_array) {
                for m in matches {
                    if let Some(branches) = m.get("branches").and_then(Value::as_array) {
                        for b in branches {
                            if let Some(props) = b.get("props").and_then(Value::as_object) {
                                allowed.extend(props.keys().cloned());
                            }
                        }
                    }
                }
            }
        }

        let group_bound = node
            .get("groupBound")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let stmt_ranges = node.get("statementRanges");
        if let Some(props) = node.get("props").and_then(Value::as_object) {
            for key in props.keys() {
                if !allowed.contains(key) && !(key == "group" && group_bound) {
                    let off =
                        offset_from_range(stmt_ranges.and_then(|r| r.get(key))).unwrap_or(start);
                    return Err(self.fail(format!("Неизвестное свойство {node_type}.{key}"), off));
                }
            }
        }
        if let Some(bindings) = node.get("bindings").and_then(Value::as_object) {
            for key in bindings.keys() {
                if !allowed.contains(key) && !(key == "group" && group_bound) {
                    let off =
                        offset_from_range(stmt_ranges.and_then(|r| r.get(key))).unwrap_or(start);
                    return Err(self.fail(format!("Неизвестное свойство {node_type}.{key}"), off));
                }
            }
        }

        let mut dependencies: BTreeSet<String> = BTreeSet::new();
        let mut collect_deps = |val: &Value| {
            for r in expression_references(val) {
                if r.starts_with("state.") {
                    dependencies.insert(r);
                }
            }
        };

        if let Some(props) = node.get("props").and_then(Value::as_object) {
            let filtered: Map<String, Value> = props
                .iter()
                .filter(|(k, _)| !matches!(k.as_str(), "context" | "contextType"))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            collect_deps(&Value::Object(filtered));
        }
        if let Some(matches) = node.get("matches") {
            collect_deps(matches);
        }
        if let Some(event_args) = node.get("eventArgs") {
            collect_deps(event_args);
        }
        if node_index == 0 {
            if let Some(defaults) = doc.get("defaults").and_then(Value::as_object) {
                let filtered: Map<String, Value> = defaults
                    .iter()
                    .filter(|(k, _)| !matches!(k.as_str(), "contextType" | "events"))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                collect_deps(&Value::Object(filtered));
            }
            for k in ["matches", "lets", "letBindings", "styles"] {
                if let Some(v) = doc.get(k) {
                    collect_deps(v);
                }
            }
        }

        let prev_defs_len = self.used_definitions.len();
        self.collect_definition(&node_type, &mut BTreeSet::new());
        if let Some(ref def) = definition {
            collect_deps(def);
        }
        if !is_layout {
            if let Some(children) = node.get("children") {
                collect_deps(children);
            }
            fn scan_supplied(value: &Value, out: &mut BTreeSet<String>) {
                match value {
                    Value::Array(items) => {
                        for item in items {
                            scan_supplied(item, out);
                        }
                    }
                    Value::Object(map) => {
                        if let Some(ty) = map.get("type").and_then(Value::as_str) {
                            out.insert(ty.to_string());
                        }
                        for (k, child) in map {
                            if !matches!(k.as_str(), "source" | "propertySources") {
                                scan_supplied(child, out);
                            }
                        }
                    }
                    _ => {}
                }
            }
            let mut supplied_types = BTreeSet::new();
            if let Some(props) = node.get("props") {
                scan_supplied(props, &mut supplied_types);
            }
            if let Some(children) = node.get("children") {
                scan_supplied(children, &mut supplied_types);
            }
            for sup_ty in supplied_types {
                let mut seen = BTreeSet::new();
                self.collect_definition(&sup_ty, &mut seen);
                for name in seen {
                    if let Some(d) = self.linked.get(&name) {
                        collect_deps(d);
                    }
                }
            }
        }
        if self.used_definitions.len() > prev_defs_len {
            let names: Vec<String> = self.used_definitions.iter().cloned().collect();
            for name in names {
                if let Some(d) = self.linked.get(&name) {
                    collect_deps(d);
                }
            }
        }

        for expr in dependencies {
            if lookup_state_path(self.state, &expr[6..]).is_some() {
                continue;
            }
            let mut segments = self.parse_state_path(Some(&expr), start)?;
            let field = segments.pop().expect("non-empty path");
            let owner = self.project(&current, &segments);
            if let Some(blueprint) = self.collection_blueprints.get(&expr).cloned() {
                let projection = self.collection_projection(&blueprint, "items")?;
                self.statements.push(format!(
                    "inner.observe_projected({node_index}, {}, &{owner}, |vm| vm.{field}.clone(), |items| {projection});",
                    rust_string(&expr[6..])
                ));
                continue;
            }
            let property = node
                .get("props")
                .and_then(Value::as_object)
                .and_then(|props| {
                    props.iter().find_map(|(k, val)| {
                        let e = val.get("expr").and_then(Value::as_str)?;
                        (e.strip_prefix('!').unwrap_or(e) == expr).then_some(k.as_str())
                    })
                });
            let off = property
                .and_then(|p| offset_from_range(stmt_ranges.and_then(|r| r.get(p))))
                .unwrap_or(start);
            let line = line_at_utf16_offset(self.source, off);
            self.statements
                .push(format!("// @forma-source {}:{line}", self.entry));
            let sample = property.and_then(|p| {
                definition
                    .as_ref()
                    .and_then(|d| d.get("defaults"))
                    .and_then(|def| def.get(p))
                    .or_else(|| self.fallback.get(p))
            });
            let contract = property.and_then(|p| {
                definition
                    .as_ref()
                    .and_then(|d| d.get("propDefinitions"))
                    .and_then(|defs| defs.get(p))
                    .and_then(|pd| pd.get("type"))
                    .and_then(Value::as_str)
            });
            let observe = if contract.is_some_and(|c| c.ends_with('?')) {
                "observe"
            } else if contract == Some("Number")
                || sample.is_some_and(Value::is_number)
                || property.is_some_and(|p| matches!(p, "x" | "y" | "padding" | "gap"))
            {
                "observe_number"
            } else if contract == Some("Bool") || sample.is_some_and(Value::is_boolean) {
                "observe_bool"
            } else if contract == Some("String") || sample.is_some_and(Value::is_string) {
                "observe_text"
            } else {
                "observe"
            };
            self.statements.push(format!(
                "inner.{observe}({node_index}, {}, &{owner}, |vm| vm.{field}.clone());",
                rust_string(&expr[6..])
            ));
        }

        if is_layout {
            let has_bindings = node
                .get("bindings")
                .and_then(Value::as_object)
                .is_some_and(|b| !b.is_empty());
            let has_events = node
                .get("events")
                .and_then(Value::as_object)
                .is_some_and(|e| !e.is_empty());
            if has_bindings || has_events {
                return Err(self.fail(
                    "Контейнер не имеет событий ввода и двусторонних свойств",
                    start,
                ));
            }
            if let Some(key) = node
                .get("props")
                .and_then(|p| p.get("key"))
                .and_then(Value::as_str)
            {
                let method = self.valid_name(&snake_case(key), start)?;
                if !self.used_names.insert(method.clone()) {
                    return Err(self.fail(format!("Конфликт Rust-метода {method}"), start));
                }
                let field = format!("container_{node_index}");
                self.fields
                    .push(format!("{field}: ::forma::binding::Context<{ty}>"));
                self.controls.push(field.clone());
                self.statements
                    .push(format!("let {field} = {current}.clone();"));
                self.methods.push(format!(
                    "pub fn {method}(&self) -> ::forma::binding::Context<{ty}> {{ self.{field}.clone() }}"
                ));
            }
            if let Some(children) = node.get_mut("children").and_then(Value::as_array_mut) {
                for child in children {
                    self.walk(child, doc, &current, &ty, loops)?;
                }
            }
            return Ok(());
        }

        let index = self.control_count;
        self.control_count += 1;
        let variable = format!("control_{index}");
        self.controls.push(variable.clone());
        self.statements.push(format!(
            "let {variable} = inner.add_control({index}, {current}.clone())?;"
        ));
        self.statements
            .push(format!("inner.bind_node({index}, {node_index});"));
        if let Some(key) = node
            .get("props")
            .and_then(|p| p.get("key"))
            .and_then(Value::as_str)
        {
            self.valid_name(key, start)?;
            let method = self.valid_name(&snake_case(key), start)?;
            if !self.used_names.insert(method.clone()) {
                return Err(self.fail(format!("Конфликт Rust-метода {method}"), start));
            }
            self.fields
                .push(format!("{variable}: ::forma::binding::Control<{ty}>"));
            self.methods.push(format!(
                "pub fn {method}(&self) -> ::forma::binding::Control<{ty}> {{ self.{variable}.clone() }}"
            ));
        } else {
            self.fields
                .push(format!("{variable}: ::forma::binding::Control<{ty}>"));
        }

        let mut bindings_map: BTreeMap<String, (String, bool)> = BTreeMap::new();
        if let Some(bindings) = node.get("bindings").and_then(Value::as_object) {
            for (k, v) in bindings {
                if let Some(expr) = v.as_str() {
                    bindings_map.insert(k.clone(), (expr.to_string(), true));
                }
            }
        }
        if let Some(props) = node.get("props").and_then(Value::as_object) {
            for (k, v) in props {
                if k != "context"
                    && let Some(expr) = v.get("expr").and_then(Value::as_str)
                    && expr.contains("state.")
                {
                    bindings_map.insert(k.clone(), (expr.to_string(), false));
                }
            }
        }

        for (key, (expr, two_way)) in bindings_map {
            let offset = offset_from_range(stmt_ranges.and_then(|r| r.get(&key))).unwrap_or(start);
            if two_way && !matches!(key.as_str(), "value" | "checked" | "selected" | "group") {
                return Err(self.fail(
                    format!("Не поддерживается двусторонняя привязка {key}"),
                    offset,
                ));
            }
            if two_way && key == "group" {
                let gv = node.get("groupValue").unwrap_or(&Value::Null);
                let literal = match gv {
                    Value::String(s) => {
                        format!(
                            "::forma::form_document::Value::Text({}.into())",
                            rust_string(s)
                        )
                    }
                    Value::Number(n) => {
                        let num = n.as_f64().unwrap_or(0.0);
                        format!("::forma::form_document::Value::Number({})", format_f64(num))
                    }
                    _ => {
                        return Err(self.fail(
                            "value радиокнопки в типизированной форме должно быть строкой или числом",
                            offset,
                        ));
                    }
                };
                let head = expr.split('.').next().unwrap_or("");
                if !expr.starts_with("state.") || loops.contains_key(head) {
                    return Err(self.fail(
                        "Привязка group в типизированной форме требует поле state",
                        offset,
                    ));
                }
                let mut segments = self.parse_state_path(Some(&expr), offset)?;
                let field = segments.pop().expect("non-empty path");
                let owner = self.project(&current, &segments);
                let line = line_at_utf16_offset(self.source, offset);
                self.statements
                    .push(format!("// @forma-source {}:{line}", self.entry));
                if owner == current {
                    self.statements.push(format!(
                        "{variable}.bind_group_path(|vm| vm.{field}.clone(), {literal});"
                    ));
                } else {
                    self.statements.push(format!(
                        "{variable}.bind_group_context(&{owner}, |vm| vm.{field}.clone(), {literal});"
                    ));
                }
                continue;
            }
            let head = expr.split('.').next().unwrap_or("");
            if two_way && let Some(local) = loops.get(head) {
                let parts: Vec<String> = expr
                    .split('.')
                    .skip(1)
                    .map(|p| self.valid_name(p, offset))
                    .collect::<Result<_, _>>()?;
                if parts.is_empty() {
                    return Err(self.fail("Привязка элемента коллекции требует поле", offset));
                }
                let indices = local
                    .indices
                    .iter()
                    .map(|s| rust_string(s))
                    .collect::<Vec<_>>()
                    .join(",");
                let nested_access = local
                    .access
                    .iter()
                    .enumerate()
                    .map(|(idx, field)| {
                        format!(
                            "let item=item.{field}.get_mut(_indices[{}]).ok_or(\"Nested collection item was removed\")?;",
                            idx + 1
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                self.statements.push(format!(
                    "{variable}.bind_collection_context({}, &[{indices}], {}, &{}, |vm| vm.{}.clone(), |item,_indices,value| {{ {nested_access} item.{}=::forma::binding::decode(value)?; Ok(()) }});",
                    rust_string(&key),
                    rust_string(&expr),
                    local.owner,
                    local.field,
                    parts.join("."),
                ));
                continue;
            }
            if two_way && !expr.starts_with("state.") {
                return Err(self.fail(
                    format!("Неизвестная область двусторонней привязки {expr}"),
                    offset,
                ));
            }
            if (!matches!(key.as_str(), "value" | "text" | "disabled")
                && !(two_way && matches!(key.as_str(), "checked" | "selected")))
                || !expr.starts_with("state.")
            {
                continue;
            }
            let is_range = key == "value"
                && (node_type == "Slider"
                    || definition
                        .as_ref()
                        .and_then(|d| d.get("defaults"))
                        .and_then(|def| def.get("value"))
                        .is_some_and(Value::is_number)
                    || definition
                        .as_ref()
                        .and_then(|d| d.get("propDefinitions"))
                        .and_then(|defs| defs.get("value"))
                        .and_then(|v| v.get("type"))
                        .and_then(Value::as_str)
                        == Some("Number"));
            let method = if is_range { "range" } else { key.as_str() };
            let mut segments = self.parse_state_path(Some(&expr), offset)?;
            let field = segments.pop().expect("non-empty path");
            let owner = self.project(&current, &segments);
            let line = line_at_utf16_offset(self.source, offset);
            self.statements
                .push(format!("// @forma-source {}:{line}", self.entry));
            let extra_arg = if key == "value" {
                format!(", {two_way}")
            } else {
                String::new()
            };
            if owner == current {
                self.statements.push(format!(
                    "{variable}.bind_{method}_path(|vm| vm.{field}.clone(){extra_arg});"
                ));
            } else {
                self.statements.push(format!(
                    "{variable}.bind_{method}_context(&{owner}, |vm| vm.{field}.clone(){extra_arg});"
                ));
            }
        }

        if let Some(events) = node.get("events").and_then(Value::as_object) {
            for (event, action_val) in events {
                if !matches!(event.as_str(), "clicked" | "changed" | "submitted") {
                    return Err(self.fail(format!("Неизвестное событие {event}"), start));
                }
                let Some(action) = action_val.as_str() else {
                    continue;
                };
                if action.starts_with("state.") {
                    if node
                        .get("eventArgs")
                        .and_then(|ea| ea.get(event))
                        .and_then(Value::as_array)
                        .is_some_and(|a| !a.is_empty())
                    {
                        return Err(self.fail(
                            "Аргументы команд используйте через типизированное event",
                            start,
                        ));
                    }
                    let ev_off =
                        offset_from_range(stmt_ranges.and_then(|r| r.get(event))).unwrap_or(start);
                    let line = line_at_utf16_offset(self.source, ev_off);
                    self.statements
                        .push(format!("// @forma-source {}:{line}", self.entry));
                    let mut segments = self.parse_state_path(Some(action), start)?;
                    let method = segments.pop().expect("non-empty path");
                    let owner = self.project(&current, &segments);
                    self.statements.push(format!(
                        "{{ let context = {owner}.clone(); {variable}.on_{event}(move |_| {{ if let Some(vm) = context.get() {{ vm.{method}(); }} }}); }}"
                    ));
                    continue;
                }
                let Some(name) = action.strip_prefix("events.") else {
                    return Err(
                        self.fail("Событие ожидает events.name() или state.method()", start)
                    );
                };
                let ev_off =
                    offset_from_range(stmt_ranges.and_then(|r| r.get(event))).unwrap_or(start);
                let (expected_args, reserved_msg, arg_types) = {
                    let Some(descriptor) = self.event_types.get(name) else {
                        return Err(
                            self.fail(format!("Событие {name} не объявлено в events"), ev_off)
                        );
                    };
                    if descriptor
                        .context_type
                        .as_deref()
                        .is_some_and(|existing| existing != ty)
                    {
                        return Err(self.fail(
                            format!("Событие {name} используется с разными типами контекста"),
                            start,
                        ));
                    }
                    let reserved = descriptor
                        .args
                        .iter()
                        .find(|a| matches!(a.name.as_str(), "context" | "control_index"))
                        .map(|a| format!("Имя аргумента события зарезервировано: {}", a.name));
                    let types: Vec<String> =
                        descriptor.args.iter().map(|a| rust_string(&a.ty)).collect();
                    (descriptor.args.len(), reserved, types)
                };
                if let Some(msg) = reserved_msg {
                    return Err(self.fail(msg, start));
                }
                let args_count = node
                    .get("eventArgs")
                    .and_then(|ea| ea.get(event))
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len);
                if args_count != expected_args {
                    return Err(self.fail(
                        format!("Событие {name} ожидает {expected_args} аргументов"),
                        start,
                    ));
                }
                let types_list = arg_types.join(",");
                self.statements.push(format!(
                    "inner.declare_event({node_index}, {}, &[{types_list}]);",
                    rust_string(event)
                ));
                let descriptor = self.event_types.get_mut(name).expect("checked event");
                descriptor.context_type = Some(ty.clone());
                descriptor.controls.push((variable.clone(), event.clone()));
            }
        }

        Ok(())
    }

    fn encode_value(&self, value: &Value) -> Result<String, String> {
        if let Some(expr) = value.get("expr").and_then(Value::as_str)
            && let Some(tail) = expr.strip_prefix("state.")
            && let Some(found) = lookup_state_path(self.state, tail)
        {
            return self.encode_value(found);
        }
        match value {
            Value::Null => Ok("V::Null".into()),
            Value::Bool(b) => Ok(format!("V::Bool({b})")),
            Value::Number(n) => Ok(format!(
                "V::Number({})",
                format_f64(n.as_f64().unwrap_or(0.0))
            )),
            Value::String(s) => Ok(format!("V::Text({}.into())", rust_string(s))),
            Value::Array(arr) => {
                let items = arr
                    .iter()
                    .map(|v| self.encode_value(v))
                    .collect::<Result<Vec<_>, _>>()?
                    .join(",");
                Ok(format!("V::List(vec![{items}])"))
            }
            Value::Object(obj) => {
                if let Some(expr_obj) = obj.get("expression").and_then(Value::as_object) {
                    let kind = expr_obj.get("kind").and_then(Value::as_str).unwrap_or("");
                    let out = match kind {
                        "unary" => {
                            let op = expr_obj
                                .get("operator")
                                .and_then(Value::as_str)
                                .unwrap_or("");
                            let arg = self
                                .encode_value(expr_obj.get("argument").unwrap_or(&Value::Null))?;
                            format!("E::Unary({}.into(),{arg})", rust_string(op))
                        }
                        "binary" => {
                            let op = expr_obj
                                .get("operator")
                                .and_then(Value::as_str)
                                .unwrap_or("");
                            let left =
                                self.encode_value(expr_obj.get("left").unwrap_or(&Value::Null))?;
                            let right =
                                self.encode_value(expr_obj.get("right").unwrap_or(&Value::Null))?;
                            format!("E::Binary({}.into(),{left},{right})", rust_string(op))
                        }
                        "conditional" => {
                            let cond = self
                                .encode_value(expr_obj.get("condition").unwrap_or(&Value::Null))?;
                            let then =
                                self.encode_value(expr_obj.get("then").unwrap_or(&Value::Null))?;
                            let otherwise =
                                self.encode_value(expr_obj.get("else").unwrap_or(&Value::Null))?;
                            format!("E::Conditional({cond},{then},{otherwise})")
                        }
                        "member" => {
                            let obj_val =
                                self.encode_value(expr_obj.get("object").unwrap_or(&Value::Null))?;
                            let prop = self
                                .encode_value(expr_obj.get("property").unwrap_or(&Value::Null))?;
                            let opt = expr_obj
                                .get("optional")
                                .and_then(Value::as_bool)
                                .unwrap_or(false);
                            format!("E::Member({obj_val},{prop},{opt})")
                        }
                        "call" => {
                            let name = expr_obj.get("name").and_then(Value::as_str).unwrap_or("");
                            let args = expr_obj
                                .get("args")
                                .and_then(Value::as_array)
                                .map(|a| {
                                    a.iter()
                                        .map(|x| self.encode_value(x))
                                        .collect::<Result<Vec<_>, _>>()
                                })
                                .transpose()?
                                .unwrap_or_default()
                                .join(",");
                            format!("E::Call({}.into(),vec![{args}])", rust_string(name))
                        }
                        other => return Err(self.fail(format!("Неизвестное выражение {other}"), 0)),
                    };
                    return Ok(format!("V::Expression(Box::new({out}))"));
                }
                if let Some(parts) = obj.get("interpolation").and_then(Value::as_array) {
                    let encoded = parts
                        .iter()
                        .map(|p| self.encode_value(p))
                        .collect::<Result<Vec<_>, _>>()?
                        .join(",");
                    return Ok(format!("V::Interpolation(vec![{encoded}])"));
                }
                if let Some(expr) = obj.get("expr").and_then(Value::as_str) {
                    return Ok(format!("V::Expr({}.into())", rust_string(expr)));
                }
                if let Some(subject) = obj.get("match") {
                    let subj = self.encode_value(subject)?;
                    let branches = obj
                        .get("branches")
                        .and_then(Value::as_array)
                        .map(|arr| {
                            arr.iter()
                                .map(|b| {
                                    let pat = self
                                        .encode_value(b.get("pattern").unwrap_or(&Value::Null))?;
                                    let val =
                                        self.encode_value(b.get("value").unwrap_or(&Value::Null))?;
                                    Ok(format!("({pat},{val})"))
                                })
                                .collect::<Result<Vec<_>, String>>()
                        })
                        .transpose()?
                        .unwrap_or_default()
                        .join(",");
                    return Ok(format!("V::Match(Box::new({subj}),vec![{branches}])"));
                }
                if obj.contains_key("type")
                    && obj.contains_key("props")
                    && obj.get("children").is_some_and(Value::is_array)
                {
                    let node_str = self.encode_node(value, false, &Map::new())?;
                    return Ok(format!("V::Node(Box::new({node_str}))"));
                }
                Ok(format!("V::Object({})", self.encode_props(obj)?))
            }
        }
    }

    fn encode_props(&self, props: &Map<String, Value>) -> Result<String, String> {
        let mut entries = Vec::new();
        for (k, v) in props {
            if matches!(k.as_str(), "context" | "contextType") {
                continue;
            }
            entries.push(format!("({},{})", rust_string(k), self.encode_value(v)?));
        }
        Ok(format!(
            "::forma::form_document::props(vec![{}])",
            entries.join(",")
        ))
    }

    fn definition_defaults(&self, d: &Value) -> Map<String, Value> {
        let mut out = d
            .get("defaults")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        if let Some(contracts) = d.get("propDefinitions").and_then(Value::as_object)
            && !contracts.is_empty()
        {
            out.insert("__contracts".into(), Value::Object(contracts.clone()));
        }
        let mut merged_enums = d
            .get("enums")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        if let Some(structs) = d.get("structs").and_then(Value::as_object) {
            for (k, v) in structs {
                merged_enums.insert(k.clone(), v.clone());
            }
        }
        if !merged_enums.is_empty() {
            out.insert("__enums".into(), Value::Object(merged_enums));
        }
        for (ast_key, meta_key) in [
            ("tokens", "__tokens"),
            ("i18n", "__i18n"),
            ("styles", "__styles"),
        ] {
            if let Some(map) = d.get(ast_key).and_then(Value::as_object)
                && !map.is_empty()
            {
                out.insert(meta_key.into(), Value::Object(map.clone()));
            }
        }
        let lets = d
            .get("lets")
            .or_else(|| d.get("letBindings"))
            .and_then(Value::as_object);
        if let Some(lets) = lets
            && !lets.is_empty()
        {
            out.insert("__lets".into(), Value::Object(lets.clone()));
        }
        if let Some(matches) = d.get("matches").and_then(Value::as_array)
            && !matches.is_empty()
        {
            let mapped = encode_matches_list(matches);
            out.insert("__matches".into(), Value::Array(mapped));
        }
        out
    }

    fn encode_node(
        &self,
        node: &Value,
        scene: bool,
        root_doc: &Map<String, Value>,
    ) -> Result<String, String> {
        let node_type = node.get("type").and_then(Value::as_str).unwrap_or("");
        let form_index = node
            .get("__form_index")
            .and_then(Value::as_u64)
            .map(|n| n as usize);
        let mut props = node
            .get("props")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        if let Some(fwd) = node.get("forward").and_then(Value::as_array)
            && !fwd.is_empty()
        {
            props.insert("__forward".into(), Value::Array(fwd.clone()));
        }
        if let Some(styles) = node.get("styles").and_then(Value::as_array)
            && !styles.is_empty()
        {
            props.insert("__styles".into(), Value::Array(styles.clone()));
        }
        if let Some(matches) = node.get("matches").and_then(Value::as_array)
            && !matches.is_empty()
        {
            props.insert(
                "__matches".into(),
                Value::Array(encode_matches_list(matches)),
            );
        }
        if let Some(ea) = node.get("eventArgs").and_then(Value::as_object)
            && !ea.is_empty()
        {
            props.insert("__event_args".into(), Value::Object(ea.clone()));
        }
        if let Some(idx) = form_index {
            props.insert("__node".into(), Value::from(idx));
            if idx == 0 {
                let mut root_clean = root_doc.clone();
                if let Some(defaults) = root_clean
                    .get_mut("defaults")
                    .and_then(Value::as_object_mut)
                {
                    defaults.remove("contextType");
                    defaults.remove("events");
                }
                let scope_obj = self.definition_defaults(&Value::Object(root_clean));
                props.insert("__scope".into(), Value::Object(scope_obj));
            }
        }
        let group_bound = node
            .get("groupBound")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if let Some(bindings) = node.get("bindings").and_then(Value::as_object) {
            for (k, v) in bindings {
                if k == "group" && group_bound {
                    continue;
                }
                if let Some(expr) = v.as_str() {
                    props.insert(k.clone(), json!({ "expr": expr }));
                }
            }
        }
        if scene
            && node_type == "Slider"
            && (node.get("bindings").and_then(|b| b.get("value")).is_some()
                || !expression_references(
                    node.get("props")
                        .and_then(|p| p.get("value"))
                        .unwrap_or(&Value::Null),
                )
                .is_empty())
        {
            props.insert("__range_normalized".into(), Value::Bool(true));
        }

        let mut children = node
            .get("children")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if node_type == "If" {
            if let Some(cond) = node.get("condition") {
                props.insert("condition".into(), cond.clone());
            }
            if let Some(else_children) = node.get("elseChildren").and_then(Value::as_array)
                && !else_children.is_empty()
            {
                children.push(json!({
                    "type": "Else",
                    "props": {},
                    "children": else_children,
                }));
            }
        }
        if node_type == "For" {
            for k in ["item", "collection", "key"] {
                if let Some(v) = node.get(k) {
                    props.insert(k.into(), v.clone());
                }
            }
            if let Some(empty_children) = node.get("emptyChildren").and_then(Value::as_array)
                && !empty_children.is_empty()
            {
                children.push(json!({
                    "type": "Empty",
                    "props": {},
                    "children": empty_children,
                }));
            }
        }
        if scene
            && !LAYOUT_TYPES.contains(&node_type)
            && !matches!(node_type, "If" | "For" | "Else" | "Empty")
            && props
                .get("key")
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
            && let Some(idx) = form_index
        {
            props.insert("key".into(), Value::String(format!("__generated_{idx}")));
        }

        let children_code = children
            .iter()
            .map(|c| self.encode_node(c, scene, root_doc))
            .collect::<Result<Vec<_>, _>>()?
            .join(",");
        let action_code = if let Some(clicked) = node
            .get("events")
            .and_then(|e| e.get("clicked"))
            .and_then(Value::as_str)
        {
            let resolved = if scene {
                format!("actions.{}", clicked.rsplit('.').next().unwrap_or(clicked))
            } else {
                clicked.to_string()
            };
            format!("Some({}.into())", rust_string(&resolved))
        } else {
            "None".into()
        };
        Ok(format!(
            "N {{kind:{}.into(),props: {},children:vec![{children_code}],action:{action_code}}}",
            rust_string(node_type),
            self.encode_props(&props)?
        ))
    }
}

fn encode_matches_list(matches: &[Value]) -> Vec<Value> {
    matches
        .iter()
        .map(|m| {
            let branches: Vec<Value> = m
                .get("branches")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .map(|b| {
                            json!({
                                "pattern": b.get("pattern").unwrap_or(&Value::Null),
                                "value": b.get("props").unwrap_or(&Value::Null),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            json!({
                "match": m.get("match").unwrap_or(&Value::Null),
                "branches": branches,
            })
        })
        .collect()
}

fn collect_blueprints(node: &Value, out: &mut BTreeMap<String, Value>) {
    if node.get("type").and_then(Value::as_str) == Some("For")
        && let Some(expr) = node
            .get("collection")
            .and_then(|c| c.get("expr"))
            .and_then(Value::as_str)
        && expr.starts_with("state.")
    {
        out.insert(expr.to_string(), node.clone());
    }
    for key in ["children", "elseChildren", "emptyChildren"] {
        if let Some(arr) = node.get(key).and_then(Value::as_array) {
            for child in arr {
                collect_blueprints(child, out);
            }
        }
    }
}

fn collect_images(value: &Value, files: &BTreeMap<String, String>, used: &mut BTreeSet<String>) {
    match value {
        Value::String(s) => {
            if s.ends_with(".svg") && files.contains_key(s) {
                used.insert(s.clone());
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_images(item, files, used);
            }
        }
        Value::Object(map) => {
            for v in map.values() {
                collect_images(v, files, used);
            }
        }
        _ => {}
    }
}

fn rust_argument_type(ty: &str, form_fail: &dyn Fn(&str) -> String) -> Result<String, String> {
    if let Some(inner) = ty.strip_suffix('?') {
        return Ok(format!("Option<{}>", rust_argument_type(inner, form_fail)?));
    }
    match ty {
        "String" => Ok("String".into()),
        "Bool" | "Boolean" => Ok("bool".into()),
        "Number" | "Float" => Ok("f64".into()),
        "Int" => Ok("i64".into()),
        other => Err(form_fail(&format!(
            "Неподдерживаемый тип аргумента {other}"
        ))),
    }
}

fn extract_source_map(rust: &str) -> BTreeMap<usize, SourceLocation> {
    let mut source_map = BTreeMap::new();
    let mut pending: Option<SourceLocation> = None;
    for (idx, line) in rust.split('\n').enumerate() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("// @forma-source ")
            && let Some((file, line_str)) = rest.rsplit_once(':')
            && let Ok(src_line) = line_str.parse::<usize>()
        {
            pending = Some(SourceLocation {
                file: file.to_string(),
                line: src_line,
            });
            continue;
        }
        if !trimmed.is_empty()
            && let Some(loc) = pending.take()
        {
            source_map.insert(idx + 1, loc);
        }
    }
    source_map
}

pub(crate) fn generate_form_impl(
    files: &BTreeMap<String, String>,
    entry: &str,
    state: &Value,
    native_app: bool,
) -> Result<GeneratedForm, String> {
    if entry.contains(['\r', '\n']) {
        return Err("Путь формы не должен содержать перевод строки".into());
    }
    let source = files
        .get(entry)
        .ok_or_else(|| format!("Форма не найдена: {entry}"))?;
    let doc = parse_ui_document(source, false).map_err(|e| format!("{entry}: {e}"))?;

    let fail_plain = |message: &str, offset: usize| -> String {
        let line = line_at_utf16_offset(source, offset);
        format!("{entry}:{line}: {message}")
    };

    let form_name = doc
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if !is_valid_ident(&form_name) || RUST_RESERVED.contains(&form_name.as_str()) {
        return Err(fail_plain(
            &format!("Недопустимое Rust-имя «{form_name}»"),
            0,
        ));
    }

    let root_type = match doc
        .get("defaults")
        .and_then(|d| d.get("contextType"))
        .and_then(Value::as_str)
    {
        Some(path) if is_valid_rust_path(path) => path.to_string(),
        _ => {
            let off =
                offset_from_range(doc.get("defaultRanges").and_then(|r| r.get("contextType")))
                    .unwrap_or(0);
            return Err(fail_plain(
                "contextType ожидает Rust-тип в строке, например 'crate::CustomerVm'",
                off,
            ));
        }
    };

    if doc.get("base").and_then(Value::as_str).is_some()
        || doc
            .get("slots")
            .and_then(Value::as_object)
            .is_some_and(|s| !s.is_empty())
    {
        return Err(fail_plain(
            "Генерируемая форма не наследуется от компонента и не использует override на корне",
            0,
        ));
    }

    let mut allowed_defaults: BTreeSet<String> = ["contextType", "events"]
        .into_iter()
        .map(str::to_string)
        .collect();
    for section in ["propDefinitions", "lets", "letBindings", "stateDefinitions"] {
        if let Some(obj) = doc.get(section).and_then(Value::as_object) {
            allowed_defaults.extend(obj.keys().cloned());
        }
    }
    if let Some(defaults) = doc.get("defaults").and_then(Value::as_object) {
        for key in defaults.keys() {
            if !allowed_defaults.contains(key) {
                let off = offset_from_range(doc.get("defaultRanges").and_then(|r| r.get(key)))
                    .unwrap_or(0);
                return Err(fail_plain(
                    &format!("Неизвестное объявление формы {key}"),
                    off,
                ));
            }
        }
    }

    let mut declared_events: Vec<String> = Vec::new();
    if let Some(events_val) = doc.get("defaults").and_then(|d| d.get("events")) {
        let Some(arr) = events_val.as_array() else {
            let off = offset_from_range(doc.get("defaultRanges").and_then(|r| r.get("events")))
                .unwrap_or(0);
            return Err(fail_plain(
                "events ожидает список имён: ['saveRequested']",
                off,
            ));
        };
        for item in arr {
            let Some(name) = item.as_str() else {
                let off = offset_from_range(doc.get("defaultRanges").and_then(|r| r.get("events")))
                    .unwrap_or(0);
                return Err(fail_plain(
                    "events ожидает список имён: ['saveRequested']",
                    off,
                ));
            };
            declared_events.push(name.to_string());
        }
    }
    if let Some(ev_defs) = doc.get("eventDefinitions").and_then(Value::as_object) {
        declared_events.extend(ev_defs.keys().cloned());
    }

    let mut used_names: BTreeSet<String> =
        BUILTIN_METHODS.iter().map(|s| (*s).to_string()).collect();
    let mut event_types = BTreeMap::new();
    let mut event_order = Vec::new();
    for name in declared_events {
        if !is_valid_ident(&name) || RUST_RESERVED.contains(&name.as_str()) {
            return Err(fail_plain(&format!("Недопустимое Rust-имя «{name}»"), 0));
        }
        if event_types.contains_key(&name) {
            return Err(fail_plain(&format!("Повторное событие {name}"), 0));
        }
        let method = format!("on_{}", snake_case(&name));
        if !used_names.insert(method.clone()) {
            return Err(fail_plain(&format!("Конфликт Rust-метода {method}"), 0));
        }
        let args = doc
            .get("eventDefinitions")
            .and_then(|e| e.get(&name))
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| {
                        Some(EventArg {
                            name: item.get("name")?.as_str()?.to_string(),
                            ty: item.get("type")?.as_str()?.to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        event_order.push(name.clone());
        event_types.insert(
            name,
            EventDescriptor {
                context_type: None,
                controls: Vec::new(),
                args,
            },
        );
    }

    let nodes = doc
        .get("nodes")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if nodes.len() != 1 {
        return Err(fail_plain(
            "Форма должна содержать один корневой Frame, Row, Column, Grid или Stack",
            0,
        ));
    }
    let root_kind = nodes[0].get("type").and_then(Value::as_str).unwrap_or("");
    if !LAYOUT_TYPES.contains(&root_kind) || root_kind == "Scroll" {
        return Err(fail_plain(
            "Форма должна содержать один корневой Frame, Row, Column, Grid или Stack",
            0,
        ));
    }

    let linked = link_component_definitions(files)?;
    let fallback = fallback_defaults();
    let mut collection_blueprints = BTreeMap::new();
    collect_blueprints(&nodes[0], &mut collection_blueprints);

    let mut builder = Generator {
        entry,
        source,
        state,
        linked: &linked,
        fallback: &fallback,
        form_name: form_name.clone(),
        statements: Vec::new(),
        fields: Vec::new(),
        methods: Vec::new(),
        controls: Vec::new(),
        event_structs: Vec::new(),
        used_names,
        used_definitions: BTreeSet::new(),
        event_types,
        event_order,
        collection_blueprints,
        serial: 0,
        node_serial: 0,
        control_count: 0,
    };

    let mut root_node = nodes[0].clone();
    builder.walk(
        &mut root_node,
        &doc,
        "context",
        &root_type,
        &BTreeMap::new(),
    )?;

    let ev_order = builder.event_order.clone();
    for name in ev_order {
        let desc = builder.event_types.get(&name).expect("registered event");
        let Some(ref ctx_type) = desc.context_type else {
            return Err(builder.fail(format!("Событие {name} объявлено, но не используется"), 0));
        };
        let event_type = if desc.args.is_empty() {
            format!("::forma::binding::Event<{ctx_type}>")
        } else {
            let mut upper = String::new();
            let mut chars = name.chars();
            if let Some(first) = chars.next() {
                upper.extend(first.to_uppercase());
                upper.push_str(chars.as_str());
            }
            format!("{}{upper}Event", builder.form_name)
        };
        if !desc.args.is_empty() {
            let mut struct_fields = Vec::with_capacity(desc.args.len());
            for arg in &desc.args {
                builder.valid_name(&arg.name, 0)?;
                let rust_ty = rust_argument_type(&arg.ty, &|msg| builder.fail(msg, 0))?;
                struct_fields.push(format!("pub {}: {rust_ty}", arg.name));
            }
            builder.event_structs.push(format!(
                "pub struct {event_type} {{ pub context: ::std::rc::Rc<{ctx_type}>, pub control_index: usize, {} }}",
                struct_fields.join(",")
            ));
        }
        let body = desc
            .controls
            .iter()
            .map(|(variable, kind)| {
                let arg_expr = if desc.args.is_empty() {
                    "event".to_string()
                } else {
                    let assigns = desc
                        .args
                        .iter()
                        .enumerate()
                        .map(|(i, a)| {
                            format!(
                                "{}: ::forma::binding::decode(&event.arguments[{i}]).expect(\"validated event argument\")",
                                a.name
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(",");
                    format!("{event_type}{{{assigns},context:event.context,control_index:event.control_index}}")
                };
                format!(
                    "{{ let handler = handler.clone(); self.{variable}.on_{kind}(move |event| handler({arg_expr})); }}"
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        builder.methods.push(format!(
            "pub fn on_{}(&self, handler: impl Fn({event_type}) + 'static) {{\n        let handler = ::std::rc::Rc::new(handler);\n        {body}\n    }}",
            snake_case(&name),
        ));
    }

    let mut used_images = BTreeSet::new();
    let doc_nodes_val = doc.get("nodes").unwrap_or(&Value::Null);
    collect_images(doc_nodes_val, files, &mut used_images);
    if let Some(defaults) = doc.get("defaults") {
        collect_images(defaults, files, &mut used_images);
    }
    let fallback_val = Value::Object(fallback.clone());
    collect_images(&fallback_val, files, &mut used_images);
    for name in &builder.used_definitions {
        if let Some(def) = linked.get(name) {
            collect_images(def, files, &mut used_images);
        }
    }

    let mut image_entries = Vec::new();
    for (name, svg_src) in files {
        if name.ends_with(".svg") && used_images.contains(name) {
            image_entries.push(svg_entry_rust(name, svg_src)?);
        }
    }

    let root_obj = doc.as_object().cloned().unwrap_or_default();
    let encoded_root = builder.encode_node(&root_node, true, &root_obj)?;

    let mut def_entries = Vec::new();
    for (name, def) in &linked {
        if !builder.used_definitions.contains(name) {
            continue;
        }
        let def_props = builder.encode_props(&builder.definition_defaults(def))?;
        let def_nodes = def
            .get("nodes")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .map(|n| builder.encode_node(n, false, &Map::new()))
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?
            .unwrap_or_default()
            .join(",");
        def_entries.push(format!(
            "({}.into(),D {{defaults: {def_props},nodes:vec![{def_nodes}]}})",
            rust_string(name)
        ));
    }
    let fallback_code = builder.encode_props(&fallback)?;

    let program = format!(
        "{{ #[allow(unused_imports)] use ::forma::form_document::{{Value as V,Expression as E,Node as N,Definition as D,Document}}; Document {{name:{}.into(),root:{encoded_root},definitions: ::std::collections::BTreeMap::from([{}]),fallback: {fallback_code},images: ::std::collections::BTreeMap::from([{}]),states:vec![] }} }}",
        rust_string(&builder.form_name),
        def_entries.join(","),
        image_entries.join(",")
    );

    let mut allowed_prop_keys = BTreeSet::new();
    if let Some(defaults) = doc.get("defaults").and_then(Value::as_object) {
        for k in defaults.keys() {
            if !matches!(k.as_str(), "contextType" | "events") {
                allowed_prop_keys.insert(k.clone());
            }
        }
    }
    if let Some(prop_defs) = doc.get("propDefinitions").and_then(Value::as_object) {
        allowed_prop_keys.extend(prop_defs.keys().cloned());
    }
    if let Some(matches) = doc.get("matches").and_then(Value::as_array) {
        for m in matches {
            if let Some(branches) = m.get("branches").and_then(Value::as_array) {
                for b in branches {
                    if let Some(props) = b.get("props").and_then(Value::as_object) {
                        allowed_prop_keys.extend(props.keys().cloned());
                    }
                }
            }
        }
    }
    let allowed_props_code = allowed_prop_keys
        .iter()
        .map(|k| rust_string(k))
        .collect::<Vec<_>>()
        .join(",");

    let clean_entry = entry.replace(['\r', '\n'], " ");
    let mut rust = String::new();
    writeln!(rust, "// @generated from {clean_entry}. Do not edit.").unwrap();
    writeln!(rust, "{}", builder.event_structs.join("\n")).unwrap();
    writeln!(rust, "pub struct {} {{", builder.form_name).unwrap();
    writeln!(rust, "    inner: ::forma::binding::Form,").unwrap();
    writeln!(rust, "    context: ::forma::binding::Context<{root_type}>,").unwrap();
    if !builder.fields.is_empty() {
        for field in &builder.fields {
            writeln!(rust, "    {field},").unwrap();
        }
    }
    writeln!(rust, "}}").unwrap();
    writeln!(rust, "impl {} {{", builder.form_name).unwrap();
    writeln!(
        rust,
        "    pub fn new(value: ::std::rc::Rc<{root_type}>) -> Result<Self, String> {{ Self::with_properties(value, ::forma::form_document::Properties::new()) }}"
    )
    .unwrap();
    writeln!(
        rust,
        "    pub fn with_properties(value: ::std::rc::Rc<{root_type}>, properties: ::forma::form_document::Properties) -> Result<Self, String> {{"
    )
    .unwrap();
    writeln!(
        rust,
        "        let context = ::forma::binding::Context::new(value);"
    )
    .unwrap();
    writeln!(rust, "        let mut program = {program};").unwrap();
    writeln!(
        rust,
        "        let allowed_properties: &[&str] = &[{allowed_props_code}];"
    )
    .unwrap();
    writeln!(
        rust,
        "        for name in properties.keys() {{ if !allowed_properties.contains(&name.as_str()) {{ return Err(format!(\"Unknown form property {{}}\", name)); }} }}"
    )
    .unwrap();
    writeln!(
        rust,
        "        if let Some(::forma::form_document::Value::Object(scope)) = program.root.props.get_mut(\"__scope\") {{ scope.extend(properties); }}"
    )
    .unwrap();
    writeln!(
        rust,
        "        let mut inner = ::forma::binding::Form::from_document(program)?;"
    )
    .unwrap();
    for stmt in &builder.statements {
        writeln!(rust, "        {stmt}").unwrap();
    }
    writeln!(rust, "        inner.sync()?;").unwrap();
    let controls_list = builder.controls.join(", ");
    if controls_list.is_empty() {
        writeln!(rust, "        Ok(Self {{ inner, context }})").unwrap();
    } else {
        writeln!(
            rust,
            "        Ok(Self {{ inner, context, {controls_list} }})"
        )
        .unwrap();
    }
    writeln!(rust, "    }}").unwrap();
    writeln!(
        rust,
        "    pub fn context(&self) -> ::forma::binding::Context<{root_type}> {{ self.context.clone() }}"
    )
    .unwrap();
    writeln!(
        rust,
        "    pub fn set_context(&mut self, value: ::std::rc::Rc<{root_type}>) -> Result<(), String> {{ self.context.set(value); self.inner.sync() }}"
    )
    .unwrap();
    writeln!(
        rust,
        "    pub fn clear_context(&mut self) -> Result<(), String> {{ self.context.clear(); self.inner.sync() }}"
    )
    .unwrap();
    writeln!(
        rust,
        "    pub fn sync(&mut self) -> Result<(), String> {{ self.inner.sync() }}"
    )
    .unwrap();
    writeln!(
        rust,
        "    pub fn is_dirty(&self) -> bool {{ self.inner.is_dirty() }}"
    )
    .unwrap();
    writeln!(
        rust,
        "    pub fn on_dirty(&self, callback: impl Fn() + 'static) {{ self.inner.on_dirty(callback); }}"
    )
    .unwrap();
    writeln!(
        rust,
        "    pub fn runtime(&mut self) -> Result<&::forma::Runtime, String> {{ self.inner.sync()?; Ok(self.inner.runtime()) }}"
    )
    .unwrap();
    writeln!(
        rust,
        "    pub fn update<R>(&mut self, input: impl FnOnce(&mut ::forma::Runtime) -> R) -> Result<R, String> {{ self.inner.update(input) }}"
    )
    .unwrap();
    for method in &builder.methods {
        writeln!(rust, "    {method}").unwrap();
    }
    writeln!(rust, "}}").unwrap();

    if native_app {
        writeln!(
            rust,
            "#[cfg(feature = \"native\")]\nimpl ::forma::native_window::NativeApp for {form_name} {{\n    fn inspect(&self, observer: &mut dyn FnMut(&::forma::Runtime)) -> Result<(), String> {{\n        observer(self.inner.runtime());\n        Ok(())\n    }}\n    fn update(&mut self, input: &mut dyn FnMut(&mut ::forma::Runtime)) -> Result<(), String> {{\n        {form_name}::update(self, |runtime| input(runtime))\n    }}\n    fn on_dirty(&self, callback: Box<dyn Fn()>) -> Result<(), String> {{\n        {form_name}::on_dirty(self, callback);\n        Ok(())\n    }}\n}}"
        )
        .unwrap();
    }

    let source_map = extract_source_map(&rust);
    Ok(GeneratedForm {
        rust,
        source_map,
        form_name: builder.form_name,
    })
}
