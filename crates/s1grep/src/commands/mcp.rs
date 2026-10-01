use std::io::{BufRead, Write};
use std::path::PathBuf;

use clap::Args;
use serde_json::{Value, json};

use crate::backend::SearchBackend;
use crate::models::ModelDirectory;
use crate::report::TextReport;
use crate::service::SearchRequest;

#[derive(Args)]
pub struct McpCommand {
    /// Repository searched when a call names no path (default: the folder the agent starts s1grep in)
    #[arg(long = "root", value_name = "PATH")]
    default_root: Option<PathBuf>,
    #[arg(long, hide = true)]
    threads: Option<usize>,
    #[command(flatten)]
    models: ModelDirectory,
}

/// A Model Context Protocol server on stdin/stdout with one tool, `search_code`. The models load on the first call,
/// or not at all when `s1grep serve` is running.
struct McpServer {
    backend: SearchBackend,
    default_root: PathBuf,
}

impl McpServer {
    const PROTOCOL_VERSIONS: [&'static str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];
    const TOOL: &'static str = "search_code";
    const PREVIEW_LINES: usize = 40;
    const INSTRUCTIONS: &'static str = "s1grep finds functions by what they do, from a description in English or \
        Spanish, and returns their file, lines and code. Use it when you know the behaviour but not where it lives; \
        use grep for exact names or strings. It reads Python repositories.";

    fn handle(&mut self, message: &Value) -> Option<Value> {
        let id = message.get("id").cloned();
        let method = message.get("method").and_then(Value::as_str).unwrap_or_default();
        let result = match method {
            "initialize" => Ok(self.initialize(message)),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(Self::tools()),
            "tools/call" => Ok(self.call(message.get("params").unwrap_or(&Value::Null))),
            _ if id.is_none() => return None,
            _ => Err(json!({"code": -32601, "message": format!("unknown method {method}")})),
        };
        let id = id?;
        Some(match result {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err(error) => json!({"jsonrpc": "2.0", "id": id, "error": error}),
        })
    }

    fn initialize(&self, message: &Value) -> Value {
        let requested = message
            .pointer("/params/protocolVersion")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let version = Self::PROTOCOL_VERSIONS
            .iter()
            .find(|known| **known == requested)
            .unwrap_or(&Self::PROTOCOL_VERSIONS[0]);
        json!({
            "protocolVersion": version,
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "s1grep", "version": env!("CARGO_PKG_VERSION")},
            "instructions": Self::INSTRUCTIONS,
        })
    }

    fn tools() -> Value {
        json!({"tools": [{
            "name": Self::TOOL,
            "title": "Search code by what it does",
            "description": "Find the functions that do what the query describes, in English or Spanish, ranked by a \
                local judge model. Returns each function's file, lines, match probability and code. Python only.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "What the code does, e.g. \"where do we retry a failed payment\""},
                    "path": {"type": "string", "description": "Repository or folder to search; defaults to the project root"},
                    "top": {"type": "integer", "minimum": 1, "maximum": SearchRequest::MAXIMUM_TOP, "description": "Results to return (default 5)"},
                    "include_tests": {"type": "boolean", "description": "Also search tests/ and migrations/"}
                },
                "required": ["query"]
            },
            "annotations": {"readOnlyHint": true, "openWorldHint": false}
        }]})
    }

    fn call(&mut self, params: &Value) -> Value {
        if params.get("name").and_then(Value::as_str) != Some(Self::TOOL) {
            return Self::failure("unknown tool; the only tool is search_code");
        }
        let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);
        let Some(query) = arguments.get("query").and_then(Value::as_str) else {
            return Self::failure("query is required");
        };
        let root = match arguments.get("path").and_then(Value::as_str) {
            Some(path) => self.default_root.join(path),
            None => self.default_root.clone(),
        };
        let top = arguments
            .get("top")
            .and_then(Value::as_u64)
            .map_or(SearchRequest::DEFAULT_TOP, |top| top as usize);
        let include_tests = arguments.get("include_tests").and_then(Value::as_bool).unwrap_or(false);
        let outcome =
            SearchRequest::new(query, &root, top, 5, include_tests).and_then(|request| self.backend.search(&request));
        match outcome {
            Ok((response, _)) => {
                let text = TextReport {
                    response: &response,
                    preview_lines: Self::PREVIEW_LINES,
                    color: false,
                }
                .render();
                json!({
                    "content": [{"type": "text", "text": format!("Results for {:?} in {}:\n\n{text}", response.query, response.root.display())}],
                    "structuredContent": serde_json::to_value(&response).unwrap_or(Value::Null),
                    "isError": false,
                })
            }
            Err(error) => Self::failure(&format!("{error:#}")),
        }
    }

    fn failure(message: &str) -> Value {
        json!({"content": [{"type": "text", "text": message}], "isError": true})
    }
}

impl McpCommand {
    pub fn run(self) -> anyhow::Result<()> {
        let default_root = match self.default_root {
            Some(root) => root,
            None => std::env::current_dir()?,
        };
        let mut server = McpServer {
            backend: SearchBackend::new(self.models, self.threads, true),
            default_root,
        };
        let stdin = std::io::stdin();
        let mut stdout = std::io::stdout().lock();
        for line in stdin.lock().lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let reply = match serde_json::from_str::<Value>(&line) {
                Ok(message) => server.handle(&message),
                Err(_) => {
                    Some(json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": "parse error"}}))
                }
            };
            if let Some(reply) = reply {
                writeln!(stdout, "{reply}")?;
                stdout.flush()?;
            }
        }
        Ok(())
    }
}
