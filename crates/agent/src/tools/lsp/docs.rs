use std::path::Path;

use serde_json::json;

use crate::language;
use summonr_core::error::{Error, Result};

use super::Conn;
use super::proto::file_uri;

pub(super) async fn did_open(conn: &mut Conn, path: &Path) -> Result<()> {
    let uri = file_uri(path)?;
    let fresh = stamp_of(path);
    if let (Some((_, _, stamp)), Some(fresh)) = (conn.docs.get(&uri), fresh)
        && *stamp == fresh
    {
        return Ok(());
    }
    let text = std::fs::read_to_string(path)
        .map_err(|error| Error::Tool(format!("cannot read {}: {error}", path.display())))?;
    let language_id = path
        .extension()
        .and_then(|extension| extension.to_str())
        .and_then(|extension| conn.languages.get(extension))
        .map_or_else(|| language::language_id(path), String::as_str)
        .to_string();
    match conn.docs.get(&uri) {
        Some((held, version, stamp)) if *held == text => {
            let entry = (text, *version, fresh.unwrap_or(*stamp));
            conn.docs.insert(uri, entry);
            Ok(())
        }
        Some((_, version, stamp)) => {
            let entry = (text.clone(), *version + 1, fresh.unwrap_or(*stamp));
            conn.notify(
                "textDocument/didChange",
                json!({
                    "textDocument": { "uri": uri, "version": entry.1 },
                    "contentChanges": [{ "text": text }]
                }),
            )
            .await?;
            conn.docs.insert(uri, entry);
            Ok(())
        }
        None => {
            conn.notify(
                "textDocument/didOpen",
                json!({
                    "textDocument": {
                        "uri": uri,
                        "languageId": language_id,
                        "version": 1,
                        "text": text
                    }
                }),
            )
            .await?;
            conn.docs.insert(uri, (text, 1, fresh.unwrap_or_default()));
            Ok(())
        }
    }
}

fn stamp_of(path: &Path) -> Option<(u64, u64)> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    Some((
        meta.len(),
        meta.modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_nanos() as u64,
    ))
}
