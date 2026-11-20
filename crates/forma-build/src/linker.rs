//! Pure-Rust `.ui` component linker, mirroring `src/compiler/link.js` and `src/language.js`.

use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use themoretheless_tokenizer_forma::{parse, parse_with_options, semantics::validate_type_name};

const BUILTIN_TEXT: &str = r#"component Text {
  width: 260; height: 30; text: ''; fontSize: 16; color: #ffffff; wrap: false;
  background: #00000000; borderWidth: 0; radius: 0;
  Rectangle { background: props.background;
    Text { text: props.text; color: props.color; fontSize: props.fontSize; wrap: props.wrap; }
  }
}"#;

const BUILTIN_TEXT_INPUT: &str = r#"component TextInput {
  width: 240; height: 38; value: ''; placeholder: ''; multiline: false;
  text: props.value; fontSize: 15; color: #ffffff; placeholderColor: #999999;
  background: #00000000; borderWidth: 0; radius: 0;
  Rectangle { background: props.background;
    TextInput { forward props { value, placeholder, multiline, fontSize, color, placeholderColor }; }
    PointerArea { clicked -> events.clicked(); }
  }
}"#;

const BUILTIN_CONTAINER_AREA: &str = r#"component ContainerArea {
  width: 0; height: 0;
  Rectangle { background: #00000000; PointerArea { clicked -> events.clicked(); } }
}"#;

fn builtin_source(name: &str) -> Option<&'static str> {
    match name {
        "Text" => Some(BUILTIN_TEXT),
        "TextInput" => Some(BUILTIN_TEXT_INPUT),
        "ContainerArea" => Some(BUILTIN_CONTAINER_AREA),
        _ => None,
    }
}

pub(crate) fn fallback_defaults() -> Map<String, Value> {
    let mut map = Map::new();
    map.insert("width".into(), json!(260));
    map.insert("height".into(), json!(70));
    map.insert("text".into(), json!(""));
    map.insert("fontSize".into(), json!(20));
    map.insert("color".into(), json!({ "expr": "#14213b" }));
    map.insert("radius".into(), json!(16));
    map.insert("disabled".into(), json!(false));
    map.insert("background".into(), json!({ "expr": "#8ca5ff" }));
    map.insert("hoverBackground".into(), json!({ "expr": "#a8baff" }));
    map.insert("pressedBackground".into(), json!({ "expr": "#6a83da" }));
    map.insert("disabledBackground".into(), json!({ "expr": "#596273" }));
    map.insert("borderWidth".into(), json!(1));
    map.insert("borderColor".into(), json!({ "expr": "#bed0ff" }));
    map.insert("focusBorderColor".into(), json!({ "expr": "#ffffff" }));
    map.insert("transitionDuration".into(), json!({ "expr": "140ms" }));
    map
}

pub(crate) fn content_props_for(kind: &str) -> Option<&'static [&'static str]> {
    Some(match kind {
        "Frame" => &[
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
            "transition",
            "columns",
            "rows",
            "gap",
            "padding",
            "clip",
            "radius",
            "capture",
        ],
        "Row" | "Column" | "Stack" => &[
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
            "transition",
            "gap",
            "padding",
            "clip",
            "radius",
            "capture",
        ],
        "Grid" => &[
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
            "transition",
            "columns",
            "rows",
            "gap",
            "padding",
            "clip",
            "radius",
            "capture",
        ],
        "Rectangle" => &[
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
            "transition",
            "background",
            "radius",
        ],
        "TextInput" => &[
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
            "transition",
            "value",
            "placeholder",
            "color",
            "placeholderColor",
            "fontSize",
            "multiline",
        ],
        "Text" => &[
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
            "transition",
            "text",
            "color",
            "fontSize",
            "font.size",
            "fontFamily",
            "font.family",
            "wrap",
        ],
        "Image" => &[
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
            "transition",
            "source",
            "color",
        ],
        _ => return None,
    })
}

pub(crate) fn bind_group_choices(nodes: &mut [Value]) {
    for node in nodes {
        let Some(obj) = node.as_object_mut() else {
            continue;
        };
        let path = obj
            .get("bindings")
            .and_then(|b| b.get("group"))
            .and_then(Value::as_str)
            .map(str::to_string);
        let bound = obj
            .get("groupBound")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if let Some(path) = path
            && !bound
        {
            let value = obj
                .get_mut("props")
                .and_then(Value::as_object_mut)
                .and_then(|props| props.remove("value"))
                .unwrap_or(Value::Null);
            if let Some(props) = obj.get_mut("props").and_then(Value::as_object_mut) {
                props.insert(
                    "checked".into(),
                    json!({
                        "expression": {
                            "kind": "binary",
                            "operator": "==",
                            "left": { "expr": path },
                            "right": value
                        }
                    }),
                );
            }
            obj.insert("groupValue".into(), value);
            obj.insert("groupBound".into(), Value::Bool(true));
        }
        for key in ["children", "elseChildren", "emptyChildren"] {
            if let Some(arr) = obj.get_mut(key).and_then(Value::as_array_mut) {
                bind_group_choices(arr);
            }
        }
        if let Some(branches) = obj.get_mut("branches").and_then(Value::as_array_mut) {
            for branch in branches {
                if let Some(arr) = branch.get_mut("children").and_then(Value::as_array_mut) {
                    bind_group_choices(arr);
                }
            }
        }
    }
}

pub(crate) fn parse_ui_document(source: &str, fragment: bool) -> Result<Value, String> {
    let mut doc = if fragment {
        parse_with_options(source, true)
    } else {
        parse(source)
    }
    .map_err(|err| err.to_string())?;
    if let Some(nodes) = doc.get_mut("nodes").and_then(Value::as_array_mut) {
        bind_group_choices(nodes);
    }
    Ok(doc)
}

fn resolve_relative_project_path(base_path: &str, rel: &str) -> Result<String, String> {
    if rel.starts_with('/') || rel.contains('\\') {
        return Err("Требуется относительный путь проекта".into());
    }
    let mut parts: Vec<&str> = match base_path.rfind('/') {
        Some(idx) => base_path[..idx]
            .split('/')
            .filter(|p| !p.is_empty())
            .collect(),
        None => Vec::new(),
    };
    for part in rel.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            if parts.pop().is_none() {
                return Err("Выход за пределы проекта".into());
            }
        } else {
            parts.push(part);
        }
    }
    Ok(parts.join("/"))
}

fn substitute_base_slot(value: &mut Value, key: &str, base: &Value) -> Result<(), String> {
    if let Some(expr) = value.get("expr").and_then(Value::as_str)
        && expr.starts_with("base.")
    {
        if expr != format!("base.{key}") {
            return Err(format!("В override {key} допустим только base.{key}"));
        }
        *value = base.clone();
        return Ok(());
    }
    match value {
        Value::Array(arr) => {
            for item in arr {
                substitute_base_slot(item, key, base)?;
            }
        }
        Value::Object(map) => {
            for child in map.values_mut() {
                substitute_base_slot(child, key, base)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn collect_presenters(nodes: &[Value], keys: &mut BTreeSet<String>) -> Result<(), String> {
    for node in nodes {
        let Some(obj) = node.as_object() else {
            continue;
        };
        if obj.get("type").and_then(Value::as_str) == Some("ContentPresenter") {
            let key = obj
                .get("props")
                .and_then(|p| p.get("key"))
                .and_then(Value::as_str)
                .filter(|k| !k.is_empty())
                .ok_or_else(|| "ContentPresenter требует key".to_string())?;
            if !keys.insert(key.to_string()) {
                return Err(format!("Повторная точка расширения {key}"));
            }
        }
        for field in ["children", "elseChildren", "emptyChildren"] {
            if let Some(children) = obj.get(field).and_then(Value::as_array) {
                collect_presenters(children, keys)?;
            }
        }
        if let Some(props) = obj.get("props").and_then(Value::as_object) {
            for prop in props.values() {
                collect_presenters_in_value(prop, keys)?;
            }
        }
    }
    Ok(())
}

fn collect_presenters_in_value(value: &Value, keys: &mut BTreeSet<String>) -> Result<(), String> {
    match value {
        Value::Array(items) => collect_presenters(items, keys)?,
        Value::Object(map) => {
            if map.get("type").is_some() {
                collect_presenters(std::slice::from_ref(value), keys)?;
            } else {
                for child in map.values() {
                    collect_presenters_in_value(child, keys)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn patch_properties(target: &mut Value, patch: &Value, key: &str) -> Result<(), String> {
    let patch_nonempty_map = |k: &str| {
        patch
            .get(k)
            .and_then(Value::as_object)
            .is_some_and(|m| !m.is_empty())
    };
    let patch_nonempty_arr = |k: &str| {
        patch
            .get(k)
            .and_then(Value::as_array)
            .is_some_and(|a| !a.is_empty())
    };
    if patch.get("base").and_then(Value::as_str).is_some()
        || patch_nonempty_arr("children")
        || patch_nonempty_arr("matches")
        || patch_nonempty_arr("forward")
        || patch_nonempty_map("slots")
        || patch_nonempty_map("events")
        || patch_nonempty_map("bindings")
    {
        return Err("Блок override переопределяет только свойства".into());
    }
    let Some(patch_props) = patch.get("props").and_then(Value::as_object) else {
        return Ok(());
    };
    if patch_props.contains_key("key") {
        return Err("override не меняет key целевого элемента".into());
    }
    let target_type = target.get("type").and_then(Value::as_str).unwrap_or("");
    if let Some(allowed) = content_props_for(target_type)
        && target_type != "Rectangle"
    {
        for name in patch_props.keys() {
            if !allowed.contains(&name.as_str()) {
                return Err(format!(
                    "override {key}: узел {target_type} не поддерживает свойство {name}"
                ));
            }
        }
    }
    let Some(target_obj) = target.as_object_mut() else {
        return Ok(());
    };
    if !target_obj.contains_key("props") || !target_obj["props"].is_object() {
        target_obj.insert("props".into(), json!({}));
    }
    let target_props = target_obj
        .get_mut("props")
        .and_then(Value::as_object_mut)
        .expect("props object");
    for (k, v) in patch_props {
        target_props.insert(k.clone(), v.clone());
    }
    Ok(())
}

fn count_key_targets(value: &Value, target_key: &str) -> usize {
    match value {
        Value::Array(items) => items.iter().map(|i| count_key_targets(i, target_key)).sum(),
        Value::Object(map) => {
            let own = usize::from(
                map.get("type").is_some()
                    && map
                        .get("props")
                        .and_then(|p| p.get("key"))
                        .and_then(Value::as_str)
                        == Some(target_key),
            );
            own + map
                .values()
                .map(|v| count_key_targets(v, target_key))
                .sum::<usize>()
        }
        _ => 0,
    }
}

fn is_key_target(value: &Value, target_key: &str) -> bool {
    value.get("type").is_some()
        && value
            .get("props")
            .and_then(|p| p.get("key"))
            .and_then(Value::as_str)
            == Some(target_key)
}

fn find_key_target_mut<'a>(value: &'a mut Value, target_key: &str) -> Option<&'a mut Value> {
    if is_key_target(value, target_key) {
        return Some(value);
    }
    match value {
        Value::Array(items) => {
            for item in items {
                if let Some(found) = find_key_target_mut(item, target_key) {
                    return Some(found);
                }
            }
            None
        }
        Value::Object(map) => {
            for child in map.values_mut() {
                if let Some(found) = find_key_target_mut(child, target_key) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

fn patch_content_node(content: &mut Value, patch: &Value, key: &str) -> Result<(), String> {
    if content.get("type").is_some() && content.get("props").is_some() {
        return patch_properties(content, patch, key);
    }
    if content.get("match").is_some()
        && let Some(branches) = content.get_mut("branches").and_then(Value::as_array_mut)
    {
        for branch in branches {
            if let Some(val) = branch.get_mut("value") {
                patch_content_node(val, patch, key)?;
            }
        }
        return Ok(());
    }
    Err(format!(
        "override {key}: для изменения свойств содержимое должно быть элементом"
    ))
}

fn replace_presenter_content(nodes: &mut [Value], key: &str, replacement: &Value) -> bool {
    for node in nodes {
        let Some(obj) = node.as_object_mut() else {
            continue;
        };
        if obj.get("type").and_then(Value::as_str) == Some("ContentPresenter")
            && obj
                .get("props")
                .and_then(|p| p.get("key"))
                .and_then(Value::as_str)
                == Some(key)
        {
            if let Some(props) = obj.get_mut("props").and_then(Value::as_object_mut) {
                props.insert("content".into(), replacement.clone());
            }
            obj.insert("children".into(), Value::Array(Vec::new()));
            return true;
        }
        for field in ["children", "elseChildren", "emptyChildren"] {
            if let Some(arr) = obj.get_mut(field).and_then(Value::as_array_mut)
                && replace_presenter_content(arr, key, replacement)
            {
                return true;
            }
        }
        if let Some(props) = obj.get_mut("props").and_then(Value::as_object_mut) {
            for val in props.values_mut() {
                if val.get("type").is_some()
                    && replace_presenter_content(std::slice::from_mut(val), key, replacement)
                {
                    return true;
                }
            }
        }
    }
    false
}

fn presenter_base_content(presenter: &Value) -> Value {
    if let Some(content) = presenter.get("props").and_then(|p| p.get("content")) {
        return content.clone();
    }
    let children = presenter
        .get("children")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if children.len() == 1 {
        children[0].clone()
    } else {
        json!({ "type": "Frame", "props": {}, "children": children })
    }
}

fn find_presenter<'a>(nodes: &'a [Value], key: &str) -> Option<&'a Value> {
    for node in nodes {
        let Some(obj) = node.as_object() else {
            continue;
        };
        if obj.get("type").and_then(Value::as_str) == Some("ContentPresenter")
            && obj
                .get("props")
                .and_then(|p| p.get("key"))
                .and_then(Value::as_str)
                == Some(key)
        {
            return Some(node);
        }
        for field in ["children", "elseChildren", "emptyChildren"] {
            if let Some(arr) = obj.get(field).and_then(Value::as_array)
                && let Some(found) = find_presenter(arr, key)
            {
                return Some(found);
            }
        }
        if let Some(props) = obj.get("props").and_then(Value::as_object) {
            for val in props.values() {
                if val.get("type").is_some()
                    && let Some(found) = find_presenter(std::slice::from_ref(val), key)
                {
                    return Some(found);
                }
            }
        }
    }
    None
}

fn merge_object_field(target: &mut Value, src: &Value, key: &str) {
    let mut merged = target
        .get(key)
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if let Some(own_obj) = src.get(key).and_then(Value::as_object) {
        for (k, v) in own_obj {
            merged.insert(k.clone(), v.clone());
        }
    }
    target[key] = Value::Object(merged);
}

fn merge_enums_and_structs(target: &mut Value, src: &Value) {
    let mut enums = target
        .get("enums")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if let Some(src_enums) = src.get("enums").and_then(Value::as_object) {
        for (k, v) in src_enums {
            enums.insert(k.clone(), v.clone());
        }
    }
    if let Some(src_structs) = src.get("structs").and_then(Value::as_object) {
        for (k, v) in src_structs {
            enums.insert(k.clone(), v.clone());
        }
    }
    if let Some(constraints) = src.get("structConstraints").and_then(Value::as_object)
        && !constraints.is_empty()
    {
        let mut combined = enums
            .get("_structConstraints")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        for (k, v) in constraints {
            combined.insert(k.clone(), v.clone());
        }
        enums.insert("_structConstraints".into(), Value::Object(combined));
    }
    target["enums"] = Value::Object(enums);
}

fn is_valid_component_ident(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub(crate) fn link_component_definitions(
    files: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, Value>, String> {
    struct Linker<'a> {
        files: &'a BTreeMap<String, String>,
        cache: BTreeMap<String, Value>,
        depths: BTreeMap<String, usize>,
        loading: Vec<String>,
    }

    impl<'a> Linker<'a> {
        fn link(&mut self, name: &str) -> Result<Value, String> {
            if let Some(cached) = self.cache.get(name) {
                let depth = self.depths.get(name).copied().unwrap_or(1);
                if self.loading.len() + depth > 32 {
                    return Err("Глубина наследования компонентов превышает 32".into());
                }
                return Ok(cached.clone());
            }
            if self.loading.iter().any(|item| item == name) {
                let mut chain = self.loading.clone();
                chain.push(name.to_string());
                return Err(format!("Цикл наследования: {}", chain.join(" → ")));
            }
            if self.loading.len() >= 32 {
                return Err("Глубина наследования компонентов превышает 32".into());
            }
            let path = format!("components/{name}.ui");
            let source = self
                .files
                .get(&path)
                .map(String::as_str)
                .or_else(|| builtin_source(name))
                .ok_or_else(|| format!("Компонент не найден: {path}"))?;
            let c = parse_ui_document(source, false)?;
            if c.get("name").and_then(Value::as_str) != Some(name) {
                return Err(format!("{path}: ожидался component {name}"));
            }
            if c.get("designs")
                .and_then(Value::as_array)
                .is_some_and(|d| !d.is_empty())
            {
                return Err(
                    "Дизайн-атрибут компонента пока не поддерживается векторным runtime".into(),
                );
            }
            self.loading.push(name.to_string());

            let base_name = c.get("base").and_then(Value::as_str).map(str::to_string);
            let mut linked = if let Some(ref base) = base_name {
                self.link(base)?
            } else {
                json!({ "nodes": [], "defaults": {} })
            };
            let own_nodes = c
                .get("nodes")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if base_name.is_some() && !own_nodes.is_empty() {
                return Err("Наследник меняет визуальное дерево через override".into());
            }
            if base_name.is_none() {
                linked["nodes"] = Value::Array(own_nodes);
            }
            for section in [
                "defaults",
                "propDefinitions",
                "stateDefinitions",
                "eventDefinitions",
                "tokens",
                "i18n",
                "styles",
                "slotDefinitions",
                "lets",
                "letBindings",
            ] {
                if c.get(section).is_some() || linked.get(section).is_some() {
                    merge_object_field(&mut linked, &c, section);
                }
            }
            if let Some(uses) = c.get("uses").and_then(Value::as_array) {
                for u in uses {
                    let u_rel = u.as_str().unwrap_or("");
                    let u_path = resolve_relative_project_path(&path, u_rel)?;
                    let u_src = self
                        .files
                        .get(&u_path)
                        .ok_or_else(|| format!("Импортируемый файл не найден: {u_path}"))?;
                    let u_doc = parse_ui_document(u_src, false)?;
                    merge_enums_and_structs(&mut linked, &u_doc);
                    for section in ["tokens", "i18n", "styles"] {
                        merge_object_field(&mut linked, &u_doc, section);
                    }
                }
            }
            merge_enums_and_structs(&mut linked, &c);
            for section in ["tokens", "i18n", "styles"] {
                merge_object_field(&mut linked, &c, section);
            }
            let mut matches = linked
                .get("matches")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if let Some(own_matches) = c.get("matches").and_then(Value::as_array) {
                matches.extend(own_matches.iter().cloned());
            }
            linked["matches"] = Value::Array(matches);

            let enums_val = linked.get("enums").cloned().unwrap_or_else(|| json!({}));
            if let Some(prop_defs) = linked.get("propDefinitions").and_then(Value::as_object) {
                for def in prop_defs.values() {
                    if let Some(ty) = def.get("type").and_then(Value::as_str) {
                        validate_type_name(ty, &enums_val).map_err(|e| e.to_string())?;
                    }
                }
            }
            if let Some(state_defs) = linked.get("stateDefinitions").and_then(Value::as_object) {
                for def in state_defs.values() {
                    if let Some(ty) = def.get("type").and_then(Value::as_str) {
                        validate_type_name(ty, &enums_val).map_err(|e| e.to_string())?;
                    }
                }
            }
            if let Some(ev_defs) = linked.get("eventDefinitions").and_then(Value::as_object) {
                for args in ev_defs.values() {
                    if let Some(arr) = args.as_array() {
                        for arg in arr {
                            if let Some(ty) = arg.get("type").and_then(Value::as_str) {
                                validate_type_name(ty, &enums_val).map_err(|e| e.to_string())?;
                            }
                        }
                    }
                }
            }
            if c.get("forward")
                .and_then(Value::as_array)
                .is_some_and(|f| !f.is_empty())
            {
                return Err(
                    "forward применяется к дочернему элементу, не к объявлению component".into(),
                );
            }

            let mut presenter_keys = BTreeSet::new();
            if let Some(nodes) = linked.get("nodes").and_then(Value::as_array) {
                collect_presenters(nodes, &mut presenter_keys)?;
            }

            if let Some(slots) = c.get("slots").and_then(Value::as_object) {
                for (key, value) in slots {
                    if value
                        .get("propertyOverride")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                    {
                        if base_name.is_none() {
                            return Err("override требует базового компонента".into());
                        }
                        let count = count_key_targets(&linked["nodes"], key);
                        if count != 1 {
                            return Err(format!(
                                "override {key}: ожидался один элемент, найдено {count}"
                            ));
                        }
                        let target = find_key_target_mut(&mut linked["nodes"], key)
                            .expect("counted target exists");
                        let patch = value.get("patch").unwrap_or(&Value::Null);
                        if target.get("type").and_then(Value::as_str) == Some("ContentPresenter") {
                            let mut content = presenter_base_content(target);
                            patch_content_node(&mut content, patch, key)?;
                            if let Some(props) =
                                target.get_mut("props").and_then(Value::as_object_mut)
                            {
                                props.insert("content".into(), content);
                            }
                            if let Some(obj) = target.as_object_mut() {
                                obj.insert("children".into(), Value::Array(Vec::new()));
                            }
                        } else {
                            patch_properties(target, patch, key)?;
                        }
                        continue;
                    }
                    if value
                        .get("fileOverride")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                    {
                        if base_name.is_none() {
                            return Err("override требует базового компонента".into());
                        }
                        let count = count_key_targets(&linked["nodes"], key);
                        if count != 1 {
                            return Err(format!(
                                "override {key}: ожидался один элемент, найдено {count}"
                            ));
                        }
                        let target_type = {
                            let target = find_key_target_mut(&mut linked["nodes"], key)
                                .expect("counted target exists");
                            target
                                .get("type")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_string()
                        };
                        let file_rel = value
                            .get("file")
                            .and_then(Value::as_str)
                            .ok_or("override from: требуется относительный путь проекта")?;
                        let resolved = resolve_relative_project_path(&path, file_rel)
                            .map_err(|_| "override from: выход за пределы проекта".to_string())?;
                        let frag_src = self
                            .files
                            .get(&resolved)
                            .ok_or_else(|| format!("Файл override не найден: {resolved}"))?;
                        let frag = parse_ui_document(frag_src, true)?;
                        let patch = frag
                            .get("nodes")
                            .and_then(Value::as_array)
                            .and_then(|arr| arr.first())
                            .ok_or_else(|| format!("Файл override пуст: {resolved}"))?;
                        let got_type = patch.get("type").and_then(Value::as_str).unwrap_or("");
                        if got_type != target_type {
                            return Err(format!(
                                "override {key}: ожидался {target_type}, получен {got_type} в {resolved}"
                            ));
                        }
                        let target = find_key_target_mut(&mut linked["nodes"], key)
                            .expect("counted target exists");
                        patch_properties(target, patch, key)?;
                        if let Some(inline_patch) = value.get("patch") {
                            patch_properties(target, inline_patch, key)?;
                        }
                        continue;
                    }
                    if !presenter_keys.contains(key) {
                        return Err(format!("Нет точки расширения {key} в {name}"));
                    }
                    if base_name.is_none() {
                        return Err("override требует базового компонента".into());
                    }
                    let base = linked
                        .get("nodes")
                        .and_then(Value::as_array)
                        .and_then(|nodes| find_presenter(nodes, key))
                        .map(presenter_base_content)
                        .ok_or_else(|| format!("Нет точки расширения {key} в {name}"))?;
                    let mut replaced = value.clone();
                    substitute_base_slot(&mut replaced, key, &base)?;
                    if let Some(nodes) = linked.get_mut("nodes").and_then(Value::as_array_mut) {
                        replace_presenter_content(nodes, key, &replaced);
                    }
                }
            }

            let depth = 1 + base_name
                .as_deref()
                .and_then(|b| self.depths.get(b).copied())
                .unwrap_or(0);
            self.loading.pop();
            self.depths.insert(name.to_string(), depth);
            self.cache.insert(name.to_string(), linked.clone());
            Ok(linked)
        }
    }

    let mut linker = Linker {
        files,
        cache: BTreeMap::new(),
        depths: BTreeMap::new(),
        loading: Vec::new(),
    };
    for builtin in ["Text", "TextInput", "ContainerArea"] {
        linker.link(builtin)?;
    }
    for path in files.keys() {
        if let Some(stem) = path
            .strip_prefix("components/")
            .and_then(|rest| rest.strip_suffix(".ui"))
            && !stem.contains('/')
            && is_valid_component_ident(stem)
        {
            linker.link(stem)?;
        }
    }
    Ok(linker.cache)
}
