use crate::error::{Error, Result};

use super::{ToolRegistry, object_string, object_usize, parse_args};

impl ToolRegistry {
    pub(super) async fn web_search(&self, arguments: &str) -> Result<String> {
        let search = self.search.as_ref().ok_or_else(|| {
            Error::Tool("web_search is unavailable in this embedded session".to_string())
        })?;
        let args: serde_json::Value = parse_args("web_search", arguments)?;
        let query = object_string(&args, "query")?;
        let count = object_usize(&args, "count");
        let freshness = args.get("freshness").and_then(serde_json::Value::as_str);
        search.search(query, count, freshness).await
    }

    pub(super) async fn web_fetch(&self, arguments: &str) -> Result<String> {
        let search = self.search.as_ref().ok_or_else(|| {
            Error::Tool("web_fetch is unavailable in this embedded session".to_string())
        })?;
        let args: serde_json::Value = parse_args("web_fetch", arguments)?;
        let url = object_string(&args, "url")?;
        search.fetch(url).await
    }
}
