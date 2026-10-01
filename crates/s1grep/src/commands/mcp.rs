use std::io::{BufRead, Write};
use std::path::PathBuf;

use clap::Args;
use serde_json::{Value, json};

use crate::backend::SearchBackend;
use crate::models::{ModelDirectory, Retriever};
use crate::progress::IndexEvent;
use crate::project::Project;
use crate::report::{CoverageNote, TextReport};
use crate::server::ServerClient;
use crate::service::{FileFilters, ProjectProgress, SearchRequest};
use crate::settings::{McpSettings, SearchSettings};

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

/// A Model Context Protocol server on stdin/stdout with two tools: `search_code` and `index_status`. Searches run in
/// the background process (started on the first call); a call that asks for progress gets notifications while the
/// project is read and indexed.
struct McpServer<Output: Write> {
    backend: SearchBackend,
    default_root: PathBuf,
    output: Output,
}

impl<Output: Write> McpServer<Output> {
    fn handle(&mut self, message: &Value) -> anyhow::Result<()> {
        let id = message.get("id").cloned();
        let method = message.get("method").and_then(Value::as_str).unwrap_or_default();
        let params = message.get("params").unwrap_or(&Value::Null);
        let result = match method {
            "initialize" => Ok(Self::initialize(params)),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(Self::tools()),
            "tools/call" => Ok(self.call(params)?),
            _ if id.is_none() => return Ok(()),
            _ => Err(json!({"code": -32601, "message": format!("unknown method {method}")})),
        };
        let Some(id) = id else { return Ok(()) };
        match result {
            Ok(result) => self.send(&json!({"jsonrpc": "2.0", "id": id, "result": result})),
            Err(error) => self.send(&json!({"jsonrpc": "2.0", "id": id, "error": error})),
        }
    }

    fn send(&mut self, message: &Value) -> anyhow::Result<()> {
        writeln!(self.output, "{message}")?;
        self.output.flush()?;
        Ok(())
    }

    fn initialize(params: &Value) -> Value {
        let requested = params
            .get("protocolVersion")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let version = McpSettings::PROTOCOL_VERSIONS
            .iter()
            .find(|known| **known == requested)
            .unwrap_or(&McpSettings::PROTOCOL_VERSIONS[0]);
        json!({
            "protocolVersion": version,
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "s1grep", "version": env!("CARGO_PKG_VERSION")},
            "instructions": McpSettings::INSTRUCTIONS,
        })
    }

    fn tools() -> Value {
        json!({"tools": [
            {
                "name": McpSettings::SEARCH_TOOL,
                "title": "Search code by what it does",
                "description": "Find the functions that do what the query describes, in English or Spanish, ranked by a \
                    local judge model. Names written like code (parse_config, sendInvoice()) and pasted error messages \
                    also match word for word. Returns each function's file, lines, match probability and code, and \
                    says when part of the repository is not indexed yet.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": {"type": "string", "description": "What the code does, e.g. \"where do we retry a failed payment\""},
                        "path": {"type": "string", "description": "Repository or folder to search; defaults to the project root"},
                        "top": {"type": "integer", "minimum": 1, "maximum": SearchSettings::MAXIMUM_TOP, "description": "Results to return (default 5)"},
                        "offset": {"type": "integer", "minimum": 0, "description": "Results to skip, to page through more (default 0)"}
                    },
                    "required": ["query"]
                },
                "annotations": {"readOnlyHint": true, "openWorldHint": false}
            },
            {
                "name": McpSettings::STATUS_TOOL,
                "title": "How far a repository is indexed",
                "description": "Reports how many functions of a repository can be searched and are fully indexed, and the \
                    time left while the background process indexes it. Use it when search_code says the repository is \
                    not fully indexed.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "Repository or folder; defaults to the project root"}
                    }
                },
                "annotations": {"readOnlyHint": true, "openWorldHint": false}
            }
        ]})
    }

    fn call(&mut self, params: &Value) -> anyhow::Result<Value> {
        let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);
        let root = match arguments.get("path").and_then(Value::as_str) {
            Some(path) => self.default_root.join(path),
            None => self.default_root.clone(),
        };
        let token = params.pointer("/_meta/progressToken").cloned();
        Ok(match params.get("name").and_then(Value::as_str) {
            Some(McpSettings::SEARCH_TOOL) => self.search(&arguments, &root, token),
            Some(McpSettings::STATUS_TOOL) => Self::status(&root),
            _ => Self::failure("unknown tool; the tools are search_code and index_status"),
        })
    }

    fn search(&mut self, arguments: &Value, root: &std::path::Path, token: Option<Value>) -> Value {
        let Some(query) = arguments.get("query").and_then(Value::as_str) else {
            return Self::failure("query is required");
        };
        let top = arguments
            .get("top")
            .and_then(Value::as_u64)
            .map_or(SearchSettings::TOP, |top| top as usize);
        let offset = arguments.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
        let output = &mut self.output;
        let backend = &mut self.backend;
        let outcome = SearchRequest::new(
            query,
            root,
            offset + top,
            SearchSettings::JUDGED,
            FileFilters::default(),
        )
        .and_then(|request| {
            backend.search(&request, &mut |event| {
                // Sent as they come, so the agent sees a long first indexing move.
                if let Some(token) = &token
                    && let Some(notification) = Self::progress(token, &event)
                {
                    let _ = writeln!(output, "{notification}").and_then(|()| output.flush());
                }
            })
        });
        match outcome {
            Ok((mut response, _)) => {
                response.results = response.results.into_iter().skip(offset).collect();
                let mut text = format!("Results for {:?} in {}:\n\n", response.query, response.root.display());
                text.push_str(
                    &TextReport {
                        response: &response,
                        preview_lines: McpSettings::PREVIEW_LINES,
                        color: false,
                    }
                    .render(),
                );
                for note in [CoverageNote::warning(&response), CoverageNote::indexing(&response)]
                    .into_iter()
                    .flatten()
                {
                    text.push_str(&format!("\n{note}"));
                }
                json!({
                    "content": [{"type": "text", "text": text}],
                    "structuredContent": serde_json::to_value(&response).unwrap_or(Value::Null),
                    "isError": false,
                })
            }
            Err(error) => Self::failure(&format!("{error:#}")),
        }
    }

    /// From the running background process when there is one (it knows the time left), else from the catalog.
    fn status(root: &std::path::Path) -> Value {
        let progress = match ServerClient::connect() {
            Some(client) => client.progress(root),
            None => Project::locate(root)
                .and_then(|project| ProjectProgress::read(&project, &project.open_store()?, Retriever::Granite, None)),
        };
        match progress {
            Ok(progress) => {
                let state = if progress.is_complete() {
                    "fully indexed".to_string()
                } else if let Some(indexing) = &progress.indexing {
                    let left = indexing
                        .searchable_seconds_left
                        .filter(|_| progress.searchable < progress.functions)
                        .or(indexing.seconds_left)
                        .map(|seconds| format!(", about {} s left", seconds.round()))
                        .unwrap_or_default();
                    format!("being indexed in the background{left}")
                } else {
                    "not fully indexed; the next search continues it".to_string()
                };
                let text = format!(
                    "{}: {} functions, {} searchable, {} fully indexed; {state}.",
                    progress.root.display(),
                    progress.functions,
                    progress.searchable,
                    progress.indexed
                );
                json!({
                    "content": [{"type": "text", "text": text}],
                    "structuredContent": serde_json::to_value(&progress).unwrap_or(Value::Null),
                    "isError": false,
                })
            }
            Err(error) => Self::failure(&format!("{error:#}")),
        }
    }

    /// A progress notification for reading and indexing events; other events are not worth one.
    fn progress(token: &Value, event: &IndexEvent) -> Option<Value> {
        let (done, total, message) = match event {
            IndexEvent::Scanning { done, files } => (*done, *files, "reading files"),
            IndexEvent::Outlining { done, total, .. } => (*done, *total, "making the repository searchable"),
            IndexEvent::Embedding { done, total, .. } => (*done, *total, "indexing functions"),
            _ => return None,
        };
        Some(json!({
            "jsonrpc": "2.0",
            "method": "notifications/progress",
            "params": {"progressToken": token, "progress": done, "total": total, "message": message}
        }))
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
            output: std::io::stdout().lock(),
        };
        for line in std::io::stdin().lock().lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<Value>(&line) {
                Ok(message) => server.handle(&message)?,
                Err(_) => server.send(
                    &json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": "parse error"}}),
                )?,
            }
        }
        Ok(())
    }
}
