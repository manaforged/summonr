use serde_json::{Value, json};

use crate::rpc::{Client, NOT_FOUND, Notice};
use summonr_core::error::{Error, Result};

pub(super) const STATELESS: &str = "2026-07-28";
const HEADER_MISMATCH: i64 = -32020;
const MISSING_CAPABILITY: i64 = -32021;
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

fn stateless(error: &Value) -> bool {
    let status = error.get("status").and_then(Value::as_u64);
    let code = error.get("code").and_then(Value::as_i64);
    let speaks = || {
        error
            .pointer("/data/supported")
            .and_then(Value::as_array)
            .is_some_and(|versions| versions.iter().any(|version| version == STATELESS))
    };
    match (status, code) {
        (Some(400 | 404), _) if speaks() => true,
        (Some(400), Some(HEADER_MISMATCH | MISSING_CAPABILITY)) | (Some(404), Some(NOT_FOUND)) => {
            true
        }
        _other => false,
    }
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
        Err(error) if !client.piped() && stateless(&error) => {
            client.agree(STATELESS);
            return Ok(None);
        }
        Err(error) => return crate::rpc::outcome("initialize", None, Some(error)).map(Some),
    };
    let version = validate_version(&result)?;
    client.agree(&version);
    client
        .notify("notifications/initialized", json!({}))
        .await?;
    Ok(Some(result))
}
