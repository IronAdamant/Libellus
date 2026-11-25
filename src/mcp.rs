//! A stdio MCP server: newline-delimited JSON-RPC 2.0.
//!
//! Hostile input never crashes it. Invalid UTF-8, malformed or deeply nested
//! JSON, oversized lines, and batches each get a JSON-RPC error and the loop
//! carries on. Unknown methods are errors. A failed tool call sets `isError`.

use crate::groups;
use crate::json::{self, Value};
use crate::tool::Tool;
use std::io::{BufRead, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;

/// Longest accepted input line, in bytes.
pub const MAX_LINE: usize = 4 * 1024 * 1024;

const PROTOCOLS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

pub struct Server {
    pub name: &'static str,
    pub version: &'static str,
    pub instructions: &'static str,
    pub tools: &'static [Tool],
    pub groups: &'static [&'static str],
    /// The project folder, used to find AGENTS.md. `None` turns groups off.
    pub project: fn() -> Option<PathBuf>,
}

enum Next {
    Eof,
    Data(Vec<u8>),
    TooLong,
}

fn read_line(r: &mut impl BufRead) -> std::io::Result<Next> {
    let mut buf: Vec<u8> = Vec::new();
    let mut too_long = false;
    loop {
        let chunk = r.fill_buf()?;
        if chunk.is_empty() {
            return Ok(if buf.is_empty() && !too_long {
                Next::Eof
            } else if too_long {
                Next::TooLong
            } else {
                Next::Data(buf)
            });
        }
        let (take, done) = match chunk.iter().position(|b| *b == b'\n') {
            Some(i) => (i + 1, true),
            None => (chunk.len(), false),
        };
        if !too_long {
            if buf.len() + take > MAX_LINE + 1 {
                too_long = true;
                buf.clear();
            } else {
                buf.extend_from_slice(&chunk[..take]);
            }
        }
        r.consume(take);
        if done {
            if too_long {
                return Ok(Next::TooLong);
            }
            while matches!(buf.last(), Some(b'\n' | b'\r')) {
                buf.pop();
            }
            return Ok(Next::Data(buf));
        }
    }
}

pub fn serve(server: &Server) -> i32 {
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    loop {
        let reply = match read_line(&mut input) {
            Ok(Next::Eof) => return 0,
            Ok(Next::TooLong) => Some(rpc_error(
                &Value::Null,
                -32600,
                &format!("line longer than {MAX_LINE} bytes"),
            )),
            Ok(Next::Data(bytes)) if bytes.iter().all(u8::is_ascii_whitespace) => None,
            Ok(Next::Data(bytes)) => handle(server, &bytes),
            Err(e) => {
                eprintln!("{}: stdin error: {e}", server.name);
                return 1;
            }
        };
        if let Some(r) = reply {
            if writeln!(out, "{r}").and_then(|_| out.flush()).is_err() {
                return 0;
            }
        }
    }
}

/// Handle one raw line. `None` means no reply (a notification).
pub fn handle(server: &Server, bytes: &[u8]) -> Option<String> {
    let text = match std::str::from_utf8(bytes) {
        Ok(t) => t,
        Err(_) => {
            return Some(rpc_error(
                &Value::Null,
                -32700,
                "parse error: input is not valid UTF-8",
            ))
        }
    };
    let msg = match json::parse(text) {
        Ok(v) => v,
        Err(e) => {
            return Some(rpc_error(
                &Value::Null,
                -32700,
                &format!("parse error: {e}"),
            ))
        }
    };
    if !matches!(msg, Value::Object(_)) {
        let why = if matches!(msg, Value::Array(_)) {
            "batches are not supported"
        } else {
            "request must be an object"
        };
        return Some(rpc_error(&Value::Null, -32600, why));
    }
    let id = msg.get("id");
    let id_ok = matches!(
        id,
        None | Some(Value::Null | Value::String(_) | Value::Number(_))
    );
    if !id_ok {
        return Some(rpc_error(
            &Value::Null,
            -32600,
            "id must be a string, number or null",
        ));
    }
    let reply_id = id.cloned().unwrap_or(Value::Null);
    let Some(method) = msg.get("method").and_then(Value::as_str) else {
        // A response or garbage from the client; there is nothing to answer.
        return if id.is_some() {
            Some(rpc_error(&reply_id, -32600, "missing method"))
        } else {
            None
        };
    };
    let params = msg.get("params");
    let is_notification = id.is_none();
    let result = match method {
        "initialize" => Ok(initialize(server, params)),
        "ping" => Ok(Value::obj(vec![])),
        "tools/list" => Ok(list(server)),
        "tools/call" => call(server, params),
        m if m.starts_with("notifications/") => return None,
        m => Err((-32601, format!("method not found: {m}"))),
    };
    if is_notification {
        return None;
    }
    Some(match result {
        Ok(v) => rpc_ok(&reply_id, v),
        Err((code, m)) => rpc_error(&reply_id, code, &m),
    })
}

fn initialize(server: &Server, params: Option<&Value>) -> Value {
    let asked = params
        .and_then(|p| p.get("protocolVersion"))
        .and_then(Value::as_str);
    let version = match asked {
        Some(v) if PROTOCOLS.contains(&v) => v,
        _ => PROTOCOLS[1],
    };
    let enabled = enabled(server);
    let mut text = server.instructions.to_string();
    if let Some(n) = &enabled.notice {
        text.push_str("\n\nNOTICE: ");
        text.push_str(n);
    }
    Value::obj(vec![
        ("protocolVersion", Value::str(version)),
        (
            "capabilities",
            Value::obj(vec![(
                "tools",
                Value::obj(vec![("listChanged", Value::Bool(false))]),
            )]),
        ),
        (
            "serverInfo",
            Value::obj(vec![
                ("name", Value::str(server.name)),
                ("version", Value::str(server.version)),
            ]),
        ),
        ("instructions", Value::String(text)),
    ])
}

fn enabled(server: &Server) -> groups::Enabled {
    let project = (server.project)();
    groups::enabled(server.tools, server.name, server.groups, project.as_deref())
}

fn list(server: &Server) -> Value {
    let enabled = enabled(server);
    let tools: Vec<Value> = enabled
        .tools
        .iter()
        .map(|t| {
            let mut d = t.description.to_string();
            if let Some(n) = &enabled.notice {
                d.push_str(" NOTICE: ");
                d.push_str(n);
            }
            Value::obj(vec![
                ("name", Value::str(t.name)),
                ("description", Value::String(d)),
                ("inputSchema", t.schema()),
            ])
        })
        .collect();
    Value::obj(vec![("tools", Value::Array(tools))])
}

fn call(server: &Server, params: Option<&Value>) -> Result<Value, (i64, String)> {
    let Some(params) = params.filter(|p| matches!(p, Value::Object(_))) else {
        return Err((-32602, "params must be an object with a tool name".into()));
    };
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return Err((-32602, "params.name must be a string".into()));
    };
    let Some(tool) = server.tools.iter().find(|t| t.name == name) else {
        return Err((-32602, format!("unknown tool: {name}")));
    };
    let enabled = enabled(server);
    if !enabled.tools.iter().any(|t| t.name == name) {
        return Ok(tool_result(Err(enabled.refusal(tool))));
    }
    let args = params.get("arguments").cloned().unwrap_or(Value::Null);
    let args = crate::tool::fill_agent_default(tool, args);
    let outcome = tool.validate(&args).and_then(|a| {
        match catch_unwind(AssertUnwindSafe(|| (tool.run)(&a))) {
            Ok(r) => r,
            Err(_) => Err("internal error: the tool panicked; nothing was reported as done".into()),
        }
    });
    Ok(tool_result(outcome))
}

fn tool_result(outcome: Result<Value, String>) -> Value {
    let (text, is_error) = match outcome {
        Ok(v) => (v.to_json(), false),
        Err(e) => (e, true),
    };
    Value::obj(vec![
        (
            "content",
            Value::Array(vec![Value::obj(vec![
                ("type", Value::str("text")),
                ("text", Value::String(text)),
            ])]),
        ),
        ("isError", Value::Bool(is_error)),
    ])
}

fn rpc_ok(id: &Value, result: Value) -> String {
    Value::obj(vec![
        ("jsonrpc", Value::str("2.0")),
        ("id", id.clone()),
        ("result", result),
    ])
    .to_json()
}

fn rpc_error(id: &Value, code: i64, message: &str) -> String {
    Value::obj(vec![
        ("jsonrpc", Value::str("2.0")),
        ("id", id.clone()),
        (
            "error",
            Value::obj(vec![
                ("code", Value::int(code)),
                ("message", Value::str(message)),
            ]),
        ),
    ])
    .to_json()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn long_lines_are_skipped_not_buffered() {
        let mut data = vec![b'x'; MAX_LINE + 10];
        data.extend_from_slice(b"\n{}\n");
        let mut r = Cursor::new(data);
        assert!(matches!(read_line(&mut r).unwrap(), Next::TooLong));
        assert!(matches!(read_line(&mut r).unwrap(), Next::Data(b) if b == b"{}"));
        assert!(matches!(read_line(&mut r).unwrap(), Next::Eof));
    }

    #[test]
    fn crlf_is_trimmed() {
        let mut r = Cursor::new(b"{}\r\n".to_vec());
        assert!(matches!(read_line(&mut r).unwrap(), Next::Data(b) if b == b"{}"));
    }
}
