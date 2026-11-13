//! Compiling one component: inheritance, slots and its definition.

use super::*;

pub(super) fn resolve_ui_file<'a>(
    files: &'a BTreeMap<String, String>,
    reference: &str,
) -> Option<(&'a str, &'a str)> {
    if let Some((k, v)) = files.get_key_value(reference) {
        return Some((k.as_str(), v.as_str()));
    }
    let trimmed = reference.trim_start_matches("./");
    if trimmed != reference {
        if let Some((k, v)) = files.get_key_value(trimmed) {
            return Some((k.as_str(), v.as_str()));
        }
    }
    for candidate in [
        format!("{reference}.ui"),
        format!("{reference}.component.ui"),
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

pub(super) fn merge_json_object(
    target: &mut serde_json::Map<String, serde_json::Value>,
    source: &serde_json::Value,
) {
    if let Some(obj) = source.as_object() {
        for (k, v) in obj {
            target.insert(k.clone(), v.clone());
        }
    }
}

pub(super) fn compile_single_component_ast(
    files: &BTreeMap<String, String>,
    file_name: &str,
    stack: &mut Vec<String>,
) -> Result<serde_json::Value, String> {
    if stack.iter().any(|s| s == file_name) {
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

pub(super) fn substitute_base_slot(
    target: &mut serde_json::Value,
    slot_name: &str,
    original_node: &serde_json::Value,
) {
    if let Some(children) = target.get_mut("children").and_then(|c| c.as_array_mut()) {
        for child in children.iter_mut() {
            let is_base_ref = child
                .get("type")
                .and_then(|t| t.as_str())
                .is_some_and(|t| t.strip_prefix("base.") == Some(slot_name));
            if is_base_ref {
                *child = original_node.clone();
            } else {
                substitute_base_slot(child, slot_name, original_node);
            }
        }
    }
}

pub(super) fn apply_slot_overrides(
    nodes: &mut [serde_json::Value],
    slots: &serde_json::Map<String, serde_json::Value>,
    files: &BTreeMap<String, String>,
) -> Result<(), String> {
    for node in nodes.iter_mut() {
        let slot_name = node
            .get("props")
            .or_else(|| node.get("properties"))
            .and_then(|p| p.get("slot").or_else(|| p.get("key")))
            .and_then(|s| {
                s.as_str()
                    .or_else(|| s.get("expr").and_then(|e| e.as_str()))
            })
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
                    || override_spec
                        .get("fileOverride")
                        .is_some_and(|v| v.is_string())
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

pub(super) fn ast_to_definition(ast: &serde_json::Value) -> Result<Definition, String> {
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
        .map(|arr| {
            arr.iter()
                .map(json_to_doc_node)
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?
        .unwrap_or_default();
    Ok(Definition { defaults, nodes })
}

pub(super) fn collect_component_names(node: &Node, out: &mut std::collections::BTreeSet<String>) {
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
        out.insert(node.kind.to_string());
    }
    for child in &node.children {
        collect_component_names(child, out);
    }
    // A component can also stand as a property value: the content of a presenter, directly or as
    // the result of a `match` arm or a conditional.
    for value in node.props.values() {
        collect_component_names_in_value(value, out);
    }
}

fn collect_component_names_in_value(value: &Value, out: &mut std::collections::BTreeSet<String>) {
    match value {
        Value::Node(node) => collect_component_names(node, out),
        Value::Match(subject, branches) => {
            collect_component_names_in_value(subject, out);
            for (pattern, result) in branches {
                collect_component_names_in_value(pattern, out);
                collect_component_names_in_value(result, out);
            }
        }
        Value::List(items) | Value::Interpolation(items) => {
            for item in items {
                collect_component_names_in_value(item, out);
            }
        }
        Value::Object(fields) => {
            for field in fields.values() {
                collect_component_names_in_value(field, out);
            }
        }
        Value::Expression(expression) => match expression.as_ref() {
            Expression::Unary(_, operand) => collect_component_names_in_value(operand, out),
            Expression::Binary(_, left, right) => {
                collect_component_names_in_value(left, out);
                collect_component_names_in_value(right, out);
            }
            Expression::Conditional(condition, then, otherwise) => {
                for part in [condition, then, otherwise] {
                    collect_component_names_in_value(part, out);
                }
            }
            Expression::Member(object, member, _) => {
                collect_component_names_in_value(object, out);
                collect_component_names_in_value(member, out);
            }
            Expression::Call(_, arguments) => {
                for argument in arguments {
                    collect_component_names_in_value(argument, out);
                }
            }
        },
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Text(_) | Value::Expr(_) => {}
    }
}

pub(super) fn builtin_definitions() -> &'static [(&'static str, Definition); 3] {
    static BUILTINS: std::sync::LazyLock<[(&'static str, Definition); 3]> =
        std::sync::LazyLock::new(|| {
            [
                (
                    "Text",
                    "component Text { width: 260; height: 30; text: ''; fontSize: 16; color: #ffffff; wrap: false; fontFamily: 'sans'; fontWeight: 'regular'; runs: null; background: #00000000; borderWidth: 0; radius: 0; Rectangle { background: props.background; Text { text: props.text; color: props.color; fontSize: props.fontSize; wrap: props.wrap; fontFamily: props.fontFamily; fontWeight: props.fontWeight; runs: props.runs; } } }",
                ),
                (
                    "TextInput",
                    "component TextInput { width: 240; height: 38; value: ''; label: ''; placeholder: ''; multiline: false; wrap: false; readOnly: false; disabled: false; text: props.value; fontSize: 15; color: #ffffff; placeholderColor: #999999; background: #00000000; borderWidth: 0; radius: 0; Rectangle { background: props.background; TextInput { forward props { value, label, placeholder, multiline, wrap, readOnly, fontSize, color, placeholderColor }; } PointerArea { clicked -> events.clicked(); } } }",
                ),
                (
                    "Slider",
                    "component Slider { width: 240; height: 32; value: 0%; min: 0; max: 1; step: 0; disabled: false; background: #00000000; borderWidth: 0; radius: 0; Rectangle { background: props.background; PointerArea { clicked -> events.clicked(); } } }",
                ),
            ]
            .map(|(name, src)| {
                let ast = themoretheless_tokenizer_forma::parse(src)
                    .expect("valid built-in control component");
                let def = ast_to_definition(&ast).expect("valid built-in control definition");
                (name, def)
            })
        });
    &BUILTINS
}
