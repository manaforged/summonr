use serde_json::{Value, json};

use crate::error::{Error, Result};
use crate::rpc::{Client, Notice};

const PROTOCOL: &str = "2025-11-25";
const SUPPORTED: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

pub(super) fn hello(name: &str, capabilities: Value) -> Value {
    json!({
        "protocolVersion": PROTOCOL,
        "capabilities": capabilities,
        "clientInfo": {"name": name, "version": env!("CARGO_PKG_VERSION")}
    })
}

pub(super) fn agreed(result: &Value) -> Result<String> {
    let version = result
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or(PROTOCOL);
    if SUPPORTED.contains(&version) {
        return Ok(version.to_string());
    }
    Err(Error::Tool(format!(
        "the server speaks MCP {version}; summonr speaks {}",
        SUPPORTED.join(", ")
    )))
}

pub(super) async fn stdio(
    client: &mut Client,
    name: &str,
    on_note: impl FnMut(Notice<'_>),
) -> Result<Value> {
    let capabilities = json!({"roots": {"listChanged": false}, "elicitation": {"form": {}}});
    let result = client
        .call("initialize", hello(name, capabilities), on_note)
        .await?;
    agreed(&result)?;
    client
        .notify("notifications/initialized", json!({}))
        .await?;
    Ok(result)
}
