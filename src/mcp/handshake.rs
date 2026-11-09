use serde_json::{Value, json};

use crate::rpc::{Client, Notice};
use summonr_core::error::{Error, Result};

const PROTOCOL: &str = "2025-11-25";
const SUPPORTED: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

fn initialize_params(name: &str) -> Value {
    json!({
        "protocolVersion": PROTOCOL,
        "capabilities": {"roots": {"listChanged": false}, "elicitation": {"form": {}}},
        "clientInfo": {"name": name, "version": env!("CARGO_PKG_VERSION")}
    })
}

fn validate_version(result: &Value) -> Result<String> {
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

pub(super) async fn initialize(
    client: &mut Client,
    name: &str,
    on_note: impl FnMut(Notice<'_>),
) -> Result<Option<Value>> {
    let result = match client
        .attempt("initialize", initialize_params(name), on_note)
        .await?
    {
        Ok(result) => result,
        Err(_refused) if !client.piped() => return Ok(None),
        Err(error) => return Err(error),
    };
    let version = validate_version(&result)?;
    client.agree(&version);
    client
        .notify("notifications/initialized", json!({}))
        .await?;
    Ok(Some(result))
}
