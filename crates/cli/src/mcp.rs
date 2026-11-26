//! Bridges MCP over stdio to the server's HTTP `/mcp` endpoint.

use crate::here;
use crate::printable;
use crate::server::connect;
use serde_json::{Value, json};
use std::io::Write;

/// An MCP server on stdin and stdout that hands each message to the server's `/mcp` and its reply
/// back: for local mode, whose address changes each time the app starts. The address is read again
/// for each message, so a restart of the app is no break. Nothing is retried here: the agent sends a
/// write again with its `request_id` (docs/agent.md#重试). Only messages go to stdout; what went
/// wrong goes to stderr, and as an error reply to a request.
pub fn mcp() -> anyhow::Result<()> {
    // Agreed on in `initialize`, and sent with every message after it.
    let mut version: Option<String> = None;
    for line in std::io::stdin().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let mut message: Value = match serde_json::from_str(&line) {
            Ok(m) => m,
            Err(e) => {
                answer(&json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": format!("不是 JSON：{e}") } }))?;
                continue;
            }
        };
        // Agents seldom say what day and time it is: through here, they are this computer's.
        if message["method"] == "tools/call" {
            let args = &mut message["params"]["arguments"];
            if args.get("today").is_none() && args.get("time").is_none() {
                here(args);
            }
        }
        match forward(&message.to_string(), version.as_deref()) {
            Ok(Some(reply)) => {
                if message["method"] == "initialize" {
                    version = reply["result"]["protocolVersion"].as_str().map(str::to_owned);
                }
                answer(&reply)?;
            }
            // A notification, or a reply to the server.
            Ok(None) => {}
            Err(e) => {
                let e = printable(&format!("{e:#}"));
                eprintln!("错误：{e}");
                if let Some(id) = message.get("id").filter(|_| message.get("method").is_some()) {
                    answer(&json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32000, "message": e } }))?;
                }
            }
        }
    }
    Ok(())
}

/// Posts one message to the server, found again for each: its reply, or none when it accepted a
/// notification.
fn forward(message: &str, version: Option<&str>) -> anyhow::Result<Option<Value>> {
    let (server, token) = connect()?;
    server.mcp(&token, message, version)
}

/// Writes a message to stdout, one line each.
fn answer(message: &Value) -> anyhow::Result<()> {
    let mut out = std::io::stdout().lock();
    writeln!(out, "{message}")?;
    out.flush()?;
    Ok(())
}
