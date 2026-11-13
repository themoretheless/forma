//! Document values and nodes to and from the JSON form of a compiled component.

use super::*;

pub(in crate::form_document) fn doc_value_to_json(v: &Value) -> Result<serde_json::Value, String> {
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

pub(in crate::form_document) fn doc_props_to_json(
    props: &Properties,
) -> Result<serde_json::Value, String> {
    let mut map = serde_json::Map::new();
    for (k, v) in props {
        if !k.starts_with("__") {
            map.insert(k.clone(), doc_value_to_json(v)?);
        }
    }
    Ok(serde_json::Value::Object(map))
}

pub(super) fn json_to_doc_value(v: &serde_json::Value) -> Result<Value, String> {
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

pub(in crate::form_document) fn json_to_doc_props(
    v: &serde_json::Value,
) -> Result<Properties, String> {
    let Some(obj) = v.as_object() else {
        return Ok(Properties::new());
    };
    let mut out = Properties::new();
    for (k, val) in obj {
        out.insert(k.clone(), json_to_doc_value(val)?);
    }
    Ok(out)
}

pub(super) fn json_to_doc_matches(v: Option<&serde_json::Value>) -> Result<Vec<Value>, String> {
    let Some(serde_json::Value::Array(arr)) = v else {
        return Ok(Vec::new());
    };
    arr.iter().map(json_to_doc_value).collect()
}

pub(super) fn json_to_doc_node(v: &serde_json::Value) -> Result<Node, String> {
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
        kind: kind.into(),
        props,
        children,
        action,
    })
}
