use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::limits::MCP_LOG_MAX;

pub(super) fn append(server: &str, level: &str, logger: Option<&str>, data: &Value) {
    let Ok(home) = crate::prompt::summonr_home() else {
        return;
    };
    let dir = home.join("logs");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let path = dir.join("mcp.log");
    if std::fs::metadata(&path).is_ok_and(|meta| meta.len() > MCP_LOG_MAX) {
        drop(std::fs::rename(&path, dir.join("mcp.log.1")));
    }
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    else {
        return;
    };
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    let source = logger.map_or_else(|| server.to_string(), |logger| format!("{server}/{logger}"));
    drop(writeln!(file, "{seconds} {source} {level} {data}"));
}
