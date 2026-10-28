use serde_json::{Value, json};

const CONTENT_MODIFIED: i64 = -32801;

#[derive(Clone, Debug)]
pub(crate) enum Peer {
    Mcp,
    Lsp(Lsp),
}

#[derive(Clone, Debug)]
pub(crate) struct Lsp {
    pub(crate) root: String,
    pub(crate) options: Value,
    pub(crate) sections: Vec<String>,
}

impl Peer {
    pub(super) fn cancels(&self) -> bool {
        matches!(self, Self::Mcp)
    }

    pub(super) fn retries(&self, error: Option<&Value>) -> bool {
        matches!(self, Self::Lsp(_))
            && error
                .and_then(|error| error.get("code"))
                .and_then(Value::as_i64)
                == Some(CONTENT_MODIFIED)
    }

    pub(super) fn label<'a>(&self, method: &'a str) -> &'a str {
        match self {
            Self::Mcp => method,
            Self::Lsp(_) => "lsp",
        }
    }
}

impl Lsp {
    pub(super) fn answer(&self, method: &str, params: &Value) -> Value {
        match method {
            "workspace/workspaceFolders" => json!([{ "uri": self.root, "name": "workspace" }]),
            "workspace/configuration" => Value::Array(
                params
                    .get("items")
                    .and_then(Value::as_array)
                    .map(|items| items.iter().map(|item| self.section(item)).collect())
                    .unwrap_or_else(|| vec![self.options.clone()]),
            ),
            _other => Value::Null,
        }
    }

    fn section(&self, item: &Value) -> Value {
        match item.get("section").and_then(Value::as_str) {
            // complexity: bounded by the two section names of one server
            Some(name) if !self.sections.iter().any(|known| known == name) => Value::Null,
            Some(_) | None => self.options.clone(),
        }
    }
}
