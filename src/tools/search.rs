use serde_json::Value;

use summonr_core::error::{Error, Result};

use super::{ToolRegistry, object_string, object_usize};

impl ToolRegistry {
    pub(super) async fn web(&self, arguments: &Value) -> Result<String> {
        match arguments.get("op").and_then(Value::as_str) {
            Some("search") => return self.web_search(arguments).await,
            Some("fetch") => return self.web_fetch(arguments).await,
            Some(operation) => {
                return Err(Error::Tool(format!("unknown web operation `{operation}`")));
            }
            None => {}
        }
        if super::args::named(arguments, "url").is_some() {
            return self.web_fetch(arguments).await;
        }
        self.web_search(arguments).await
    }

    pub(super) async fn web_search(&self, args: &Value) -> Result<String> {
        let search = self.search.as_ref().ok_or_else(|| {
            Error::Tool("web_search is unavailable in this embedded session".to_string())
        })?;
        let query = object_string(args, "query")?;
        let count = object_usize(args, "count");
        let freshness = args.get("freshness").and_then(Value::as_str);
        search.search(query, count, freshness).await
    }

    pub(super) async fn web_fetch(&self, args: &Value) -> Result<String> {
        let search = self.search.as_ref().ok_or_else(|| {
            Error::Tool("web_fetch is unavailable in this embedded session".to_string())
        })?;
        let url = object_string(args, "url")?;
        let format = crate::web::Format::parse(args.get("format").and_then(Value::as_str))?;
        search.fetch(url, format).await
    }
}
