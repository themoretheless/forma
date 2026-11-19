pub mod bridge;
pub mod protocol;

use bridge::Bridge;
use forma::mcp::McpApp;
use serde_json::{Value, json};
use std::io::{self, BufRead, Write};
use std::path::Path;

const LANGUAGE_URI: &str = "forma://language";

#[derive(Debug)]
pub struct McpServer {
    pub bridge: Bridge,
    pub app: McpApp,
}

impl McpServer {
    pub fn new(bridge: Bridge, mut app: McpApp) -> Self {
        app.set_native_manifest(Path::new(env!("CARGO_MANIFEST_DIR")).join("../forma/Cargo.toml"));
        Self { bridge, app }
    }

    pub fn serve_io<R: BufRead, W: Write>(&mut self, input: R, mut output: W) -> io::Result<()> {
        for line in input.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let response = match serde_json::from_str::<Value>(&line) {
                Ok(request) => self.handle_request(request),
                Err(error) => Some(error_response(None, -32700, &error.to_string())),
            };
            if let Some(response) = response {
                serde_json::to_writer(&mut output, &response)?;
                output.write_all(b"\n")?;
                output.flush()?;
            }
        }
        Ok(())
    }

    pub fn handle_request(&mut self, request: Value) -> Option<Value> {
        let Some(obj) = request.as_object() else {
            return Some(error_response(None, -32600, "Invalid Request"));
        };

        let valid_id = match obj.get("id") {
            Some(id) if is_valid_request_id(id) => Some(id.clone()),
            Some(_) => return Some(error_response(None, -32600, "Invalid Request")),
            None => None,
        };

        if obj.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return Some(error_response(valid_id, -32600, "Invalid Request"));
        }

        let Some(method) = obj.get("method").and_then(Value::as_str) else {
            return Some(error_response(valid_id, -32600, "Invalid Request"));
        };

        let id = valid_id?;
        let params = obj.get("params");

        let response = match method {
            "initialize" => success_response(
                id,
                json!({
                    "protocolVersion": params
                        .and_then(|params| params.get("protocolVersion"))
                        .and_then(Value::as_str)
                        .unwrap_or("2025-06-18"),
                    "capabilities": {
                        "tools": { "listChanged": false },
                        "resources": { "subscribe": false, "listChanged": false },
                    },
                    "serverInfo": {
                        "name": "forma",
                        "version": env!("CARGO_PKG_VERSION"),
                    },
                }),
            ),
            "ping" | "shutdown" => success_response(id, json!({})),
            "tools/list" => {
                success_response(id, json!({ "tools": protocol::all_tool_definitions() }))
            }
            "resources/list" => {
                let mut resources = vec![json!({
                    "uri": LANGUAGE_URI,
                    "name": "Forma language",
                    "description": "Implemented language and IDE limitations",
                    "mimeType": "text/markdown",
                })];
                resources.extend(forma::mcp::runtime_resources_json());
                success_response(id, json!({ "resources": resources }))
            }
            "resources/read" => self.resource_response(id, params),
            "tools/call" => self.tool_response(id, params),
            _ => error_response(Some(id), -32601, &format!("Method not found: {method}")),
        };
        Some(response)
    }

    fn resource_response(&self, id: Value, params: Option<&Value>) -> Value {
        let Some(uri) = params
            .and_then(|params| params.get("uri"))
            .and_then(Value::as_str)
        else {
            return error_response(Some(id), -32602, "Unknown resource URI");
        };
        if uri == LANGUAGE_URI {
            return success_response(
                id,
                json!({
                    "contents": [{
                        "uri": LANGUAGE_URI,
                        "text": include_str!("../../../README.md"),
                        "mimeType": "text/markdown",
                    }]
                }),
            );
        }
        match self.app.read_resource(uri) {
            Some(result) => success_response(id, result),
            None => error_response(Some(id), -32602, "Unknown resource URI"),
        }
    }

    fn tool_response(&mut self, id: Value, params: Option<&Value>) -> Value {
        let Some(params) = params.and_then(Value::as_object) else {
            return error_response(Some(id), -32602, "Missing tool parameters");
        };
        let Some(name) = params.get("name").and_then(Value::as_str) else {
            return error_response(Some(id), -32602, "Missing tool name");
        };

        let result = if let Some(tool) = protocol::find_tool(name) {
            match protocol::validate_arguments(tool, params.get("arguments")) {
                Ok(arguments) => self
                    .bridge
                    .call(name, arguments)
                    .map_err(|error| error.to_string()),
                Err(error) => Err(error),
            }
        } else if forma::mcp::is_runtime_tool(name) {
            match params.get("arguments") {
                None | Some(Value::Null) => self.app.call_tool(name, &json!({})),
                Some(args @ Value::Object(_)) => self.app.call_tool(name, args),
                Some(_) => Err("Tool arguments must be an object".into()),
            }
        } else {
            return error_response(Some(id), -32602, &format!("Unknown tool: {name}"));
        };

        let (text, is_error) = match result {
            Ok(result) => (
                serde_json::to_string_pretty(&result).unwrap_or_else(|_| "null".into()),
                false,
            ),
            Err(error) => (error, true),
        };
        success_response(
            id,
            json!({
                "content": [{ "type": "text", "text": text }],
                "isError": is_error,
            }),
        )
    }
}

fn is_valid_request_id(value: &Value) -> bool {
    value.is_string() || value.is_i64() || value.is_u64()
}

fn success_response(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error_response(id: Option<Value>, code: i32, message: &str) -> Value {
    match id {
        Some(id) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": code, "message": message },
        }),
        None => json!({
            "jsonrpc": "2.0",
            "error": { "code": code, "message": message },
        }),
    }
}
