use std::collections::BTreeMap;

use super::{Definition, Document, Expression, Node, Properties, Value, number, parse_svg_shapes};

pub(super) fn doc_value_to_json(v: &Value) -> Result<serde_json::Value, String> {
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

pub(super) fn doc_props_to_json(props: &Properties) -> Result<serde_json::Value, String> {
    let mut map = serde_json::Map::new();
    for (k, v) in props {
        if !k.starts_with("__") {
            map.insert(k.clone(), doc_value_to_json(v)?);
        }
    }
    Ok(serde_json::Value::Object(map))
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
                    let val = if let Some(props) = c.get("props").or_else(|| c.get("properties")) {
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

pub(super) fn json_to_doc_props(v: &serde_json::Value) -> Result<Properties, String> {
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
    if let Some(styles_val) = v.get("apply").or_else(|| v.get("styles")).or_else(|| {
        props
            .get("apply")
            .and_then(|_| v.get("props").and_then(|p| p.get("apply")))
    }) {
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
        } else if let Some(s) = styles_val
            .as_str()
            .or_else(|| styles_val.get("expr").and_then(|e| e.as_str()))
        {
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
        .and_then(|c| {
            c.as_str()
                .or_else(|| c.get("action").and_then(|a| a.as_str()))
        })
        .map(String::from);
    let mut children = v
        .get("children")
        .and_then(|c| c.as_array())
        .map(|arr| {
            arr.iter()
                .map(json_to_doc_node)
                .collect::<Result<Vec<_>, _>>()
        })
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
                    .map(|arr| {
                        arr.iter()
                            .map(json_to_doc_node)
                            .collect::<Result<Vec<_>, _>>()
                    })
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

fn merge_json_object(
    target: &mut serde_json::Map<String, serde_json::Value>,
    source: &serde_json::Value,
) {
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
                .is_some_and(|t| t.strip_prefix("base.") == Some(slot_name));
            if is_base_ref {
                *child = original_node.clone();
            } else {
                substitute_base_slot(child, slot_name, original_node);
            }
        }
    }
}

fn apply_slot_overrides(
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
        .map(|arr| {
            arr.iter()
                .map(json_to_doc_node)
                .collect::<Result<Vec<_>, _>>()
        })
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

fn builtin_definitions() -> &'static [(&'static str, Definition); 3] {
    static BUILTINS: std::sync::LazyLock<[(&'static str, Definition); 3]> =
        std::sync::LazyLock::new(|| {
            [
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

pub(super) fn compile_ui_files_to_document(
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
        .map(|arr| {
            arr.iter()
                .map(json_to_doc_node)
                .collect::<Result<Vec<_>, _>>()
        })
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
    for path in files.keys() {
        if let Some(stem) = path.strip_suffix(".component.ui") {
            pending.insert(stem.to_string());
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
    for (builtin_name, builtin_def) in builtin_definitions() {
        if !definitions.contains_key(*builtin_name) {
            definitions.insert((*builtin_name).into(), builtin_def.clone());
        }
    }

    // Merge initial state from `stateDefinitions` defaults + `designs` + `initial_state`
    let mut root_state = Properties::new();
    if let Some(state_defs) = scene_ast
        .get("stateDefinitions")
        .and_then(|s| s.as_object())
    {
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
