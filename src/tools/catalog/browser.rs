use crate::error::{Error, Result};
use crate::provider::ToolDefinition;
use crate::tools::{ToolRegistry, tool};

impl ToolRegistry {
    pub(in crate::tools) fn set_browser(&mut self, on: bool) {
        let held = self.browser.take();
        self.browser = on
            .then(|| held.or_else(|| crate::browser::Browser::discover().ok()))
            .flatten();
        if let Some(search) = &self.search {
            search.set_browser(self.browser.clone());
        }
    }

    pub(in crate::tools) async fn browse(&self, arguments: &str) -> Result<String> {
        let session = self.session.lock().ok().and_then(|held| held.clone());
        let browser = self
            .browser
            .as_ref()
            .ok_or_else(|| Error::Tool("the browser module is off".to_string()))?;
        browser
            .call(arguments, &self.workspace, session.as_deref())
            .await
    }
}

pub(super) fn browser() -> ToolDefinition {
    tool(
        "browser",
        "Drive a real Chrome that keeps its logins. Start with snapshot, then act on refs such as e5: click, fill, type, press, select, check, hover, drag, scroll, upload (workspace files), navigate, back, forward, reload, wait, tab (new|select|close). Each action returns the url, title, new tabs, dialogs, console error count, and the snapshot delta (report none|delta|full). read returns the page text, screenshot saves a PNG and returns its path, tabs lists tabs, login opens a visible window for the user to sign in, close quits Chrome.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {"type": "string", "enum": [
                    "snapshot", "read", "screenshot", "tabs", "login", "close",
                    "navigate", "back", "forward", "reload", "click", "fill", "type", "press",
                    "select", "check", "upload", "hover", "drag", "scroll", "wait", "tab"
                ]},
                "url": {"type": "string"},
                "target": {"type": "string", "description": "A ref such as e5, or a selector."},
                "text": {"type": "string"},
                "key": {"type": "string"},
                "from": {"type": "string"},
                "to": {"type": "string"},
                "delta_y": {"type": "number"},
                "checked": {"type": "boolean"},
                "files": {"type": "array", "items": {"type": "string"}},
                "by": {"type": "object", "description": "{\"value\"|\"label\"|\"index\": [..]}"},
                "for": {"type": "object", "description": "{\"selector\"|\"text\"|\"url\"|\"url_exact\"|\"url_glob\"|\"load\"|\"ms\": ..}"},
                "options": {"type": "object", "description": "Click options: button, count, modifiers, position."},
                "new": {"type": ["string", "null"]},
                "select": {"type": "string"},
                "close": {"type": "string"},
                "interactive_only": {"type": "boolean"},
                "max_chars": {"type": "integer", "minimum": 1},
                "format": {"type": "string", "enum": ["markdown", "text"]},
                "full_page": {"type": "boolean"},
                "report": {"type": "string", "enum": ["none", "delta", "full"]}
            },
            "required": ["action"],
            "additionalProperties": false
        }),
    )
}
