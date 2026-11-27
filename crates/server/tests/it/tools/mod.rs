//! The agent tools (docs/agent.md) as the API and MCP serve them, a module for each topic.

mod contract;
mod folders;
mod lists;
mod log;
mod plans;
mod repeat;
mod tasks;

use serde_json::Value;

fn numbers(list: &Value) -> Vec<i64> {
    list.as_array().unwrap().iter().map(|t| t["number"].as_i64().unwrap()).collect()
}

fn text(reply: &Value) -> &str {
    reply["text"].as_str().unwrap()
}
