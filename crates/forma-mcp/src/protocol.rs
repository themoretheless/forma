use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::sync::OnceLock;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub schema: BTreeMap<String, Field>,
    pub read_only: bool,
    #[serde(default)]
    pub destructive: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Field {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    values: Vec<String>,
    #[serde(default)]
    optional: bool,
    #[serde(default)]
    nullable: bool,
}

pub fn tools() -> &'static [Tool] {
    static TOOLS: OnceLock<Vec<Tool>> = OnceLock::new();
    TOOLS.get_or_init(|| {
        serde_json::from_str(include_str!("../../../protocol/mcp-tools.json"))
            .expect("the shared MCP tool registry must be valid")
    })
}

pub fn find_tool(name: &str) -> Option<&'static Tool> {
    tools().iter().find(|tool| tool.name == name)
}

pub fn tool_definitions() -> Vec<Value> {
    tools().iter().map(tool_definition).collect()
}

pub fn all_tool_definitions() -> Vec<Value> {
    let mut definitions = tool_definitions();
    definitions.extend(forma::mcp::runtime_tool_definitions());
    definitions
}

fn tool_definition(tool: &Tool) -> Value {
    json!({
        "name": tool.name,
        "description": tool.description,
        "inputSchema": input_schema(tool),
        "annotations": {
            "readOnlyHint": tool.read_only,
            "destructiveHint": tool.destructive,
            "openWorldHint": false,
        }
    })
}

fn input_schema(tool: &Tool) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();
    for (name, field) in &tool.schema {
        properties.insert(name.clone(), field_schema(field));
        if !field.optional {
            required.push(Value::String(name.clone()));
        }
    }
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

fn field_schema(field: &Field) -> Value {
    let scalar = || {
        json!({
            "anyOf": [
                { "type": "string" },
                { "type": "number" },
                { "type": "boolean" },
                { "type": "null" },
            ]
        })
    };
    let base = match field.kind.as_str() {
        "path" => json!({ "type": "string", "minLength": 1 }),
        "string" => json!({ "type": "string" }),
        "boolean" => json!({ "type": "boolean" }),
        "int" => json!({ "type": "integer" }),
        "nonnegativeInt" => json!({ "type": "integer", "minimum": 0 }),
        "positiveInt" => json!({ "type": "integer", "minimum": 1 }),
        "enum" => json!({ "type": "string", "enum": field.values }),
        "scalar" => scalar(),
        "stringRecord" => json!({
            "type": "object",
            "additionalProperties": { "type": "string" },
        }),
        "scalarRecord" => json!({
            "type": "object",
            "additionalProperties": scalar(),
        }),
        kind => panic!("unsupported MCP field type: {kind}"),
    };
    if field.nullable {
        json!({ "anyOf": [base, { "type": "null" }] })
    } else {
        base
    }
}

pub fn validate_arguments(tool: &Tool, arguments: Option<&Value>) -> Result<Value, String> {
    let empty = Map::new();
    let arguments = match arguments {
        None | Some(Value::Null) => &empty,
        Some(Value::Object(arguments)) => arguments,
        Some(_) => return Err("Tool arguments must be an object".into()),
    };
    for name in arguments.keys() {
        if !tool.schema.contains_key(name) {
            return Err(format!("Unknown argument: {name}"));
        }
    }
    let mut checked = Map::new();
    for (name, field) in &tool.schema {
        match arguments.get(name) {
            None if field.optional => {}
            None => return Err(format!("Missing required argument: {name}")),
            Some(value) => {
                validate_value(name, field, value)?;
                checked.insert(name.clone(), value.clone());
            }
        }
    }
    Ok(Value::Object(checked))
}

fn validate_value(name: &str, field: &Field, value: &Value) -> Result<(), String> {
    if field.nullable && value.is_null() {
        return Ok(());
    }
    let invalid = || format!("Invalid value for argument: {name}");
    match field.kind.as_str() {
        "path" if value.as_str().is_some_and(|value| !value.is_empty()) => Ok(()),
        "string" if value.is_string() => Ok(()),
        "boolean" if value.is_boolean() => Ok(()),
        "int" | "nonnegativeInt" | "positiveInt" => {
            let Some(number) = value.as_f64().filter(|number| {
                number.is_finite()
                    && number.fract() == 0.0
                    && (-9_007_199_254_740_991.0..=9_007_199_254_740_991.0).contains(number)
            }) else {
                return Err(invalid());
            };
            let minimum = match field.kind.as_str() {
                "nonnegativeInt" => 0.0,
                "positiveInt" => 1.0,
                _ => f64::NEG_INFINITY,
            };
            if number >= minimum {
                Ok(())
            } else {
                Err(invalid())
            }
        }
        "enum" => {
            if value
                .as_str()
                .is_some_and(|value| field.values.iter().any(|allowed| allowed == value))
            {
                Ok(())
            } else {
                Err(invalid())
            }
        }
        "scalar" if !value.is_array() && !value.is_object() => Ok(()),
        "stringRecord" | "scalarRecord" => {
            let Some(record) = value.as_object() else {
                return Err(invalid());
            };
            if record.values().all(|value| {
                if field.kind == "stringRecord" {
                    value.is_string()
                } else {
                    !value.is_array() && !value.is_object()
                }
            }) {
                Ok(())
            } else {
                Err(invalid())
            }
        }
        _ => Err(invalid()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_exposes_the_full_catalog() {
        assert!(tools().len() >= 45);
        let definitions = tool_definitions();
        let file_read = definitions
            .iter()
            .find(|tool| tool["name"] == "file_read")
            .unwrap();
        assert_eq!(file_read["inputSchema"]["required"], json!(["path"]));
        assert_eq!(
            file_read["inputSchema"]["properties"]["path"]["minLength"],
            1
        );
    }

    #[test]
    fn validates_required_optional_nullable_and_scalar_arguments() {
        let write = find_tool("file_write").unwrap();
        assert!(validate_arguments(write, Some(&json!({}))).is_err());
        assert!(
            validate_arguments(
                write,
                Some(&json!({ "path": "ui/A.ui", "content": "component A {}" }))
            )
            .is_ok()
        );

        let scenario = find_tool("preview_scenario").unwrap();
        assert!(validate_arguments(scenario, Some(&json!({ "name": null }))).is_ok());
        let state = find_tool("state_set").unwrap();
        assert!(
            validate_arguments(
                state,
                Some(&json!({ "values": { "text": "hello", "count": 2, "ok": true } }))
            )
            .is_ok()
        );
        assert!(validate_arguments(state, Some(&json!({ "values": { "bad": [] } }))).is_err());
    }

    #[test]
    fn validates_integer_ranges_and_unknown_arguments() {
        let select = find_tool("component_select").unwrap();
        assert!(validate_arguments(select, Some(&json!({ "start": 0 }))).is_ok());
        assert!(validate_arguments(select, Some(&json!({ "start": -1 }))).is_err());
        assert!(validate_arguments(select, Some(&json!({ "start": 1.5 }))).is_err());
        assert!(validate_arguments(select, Some(&json!({ "start": 0, "extra": true }))).is_err());
    }

    #[test]
    fn unified_catalog_combines_studio_and_runtime_tools_without_overlap() {
        let all = all_tool_definitions();
        assert_eq!(all.len(), 57);
        let names: std::collections::BTreeSet<&str> = all
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect();
        assert_eq!(names.len(), 57);
    }
}
