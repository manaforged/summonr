use super::entry::linked;
use super::*;

impl Model {
    pub(in crate::tui::sigil_view) fn start_tool(
        &mut self,
        id: String,
        name: String,
        arguments: String,
    ) {
        let arg = tool_arg(&arguments);
        self.tools.insert(id, self.entries.len());
        self.entries.push(Entry::Tool {
            name: name.clone(),
            arg: arg.clone(),
            state: ToolState::Running,
            output: String::new(),
        });
        self.live_tool = Some((name, arg));
    }

    pub(in crate::tui::sigil_view) fn finish_tool(
        &mut self,
        id: &str,
        success: bool,
        output: String,
    ) {
        if let Some(index) = self.tools.remove(id)
            && let Some(Entry::Tool {
                state,
                output: stored,
                ..
            }) = self.entries.get_mut(index)
        {
            *state = if success {
                ToolState::Ok
            } else {
                ToolState::Failed
            };
            *stored = output;
        }
        self.live_tool = Some(("thinking".to_string(), String::new()));
    }

    pub(in crate::tui::sigil_view) fn finish_assistant(&mut self, text: String) {
        let links = linked(&text);
        self.place_assistant(text);
        for path in links {
            self.show(&path);
        }
    }

    pub(in crate::tui::sigil_view) fn show_call(&mut self, arguments: &str, quiet: bool) {
        let path = serde_json::from_str::<serde_json::Value>(arguments)
            .ok()
            .and_then(|value| value.get("path")?.as_str().map(str::to_string));
        if let Some(path) = path {
            self.show_path(&path, quiet);
        }
    }

    pub(in crate::tui::sigil_view) fn show(&mut self, path: &str) {
        self.show_path(path, false);
    }

    pub(in crate::tui::sigil_view) fn show_path(&mut self, path: &str, quiet: bool) {
        let full = if std::path::Path::new(path).is_absolute() {
            std::path::PathBuf::from(path)
        } else {
            std::env::current_dir()
                .map(|dir| dir.join(path))
                .unwrap_or_else(|_| std::path::PathBuf::from(path))
        };
        let bytes = std::fs::read(&full).map_err(|error| error.to_string());
        if quiet
            && !bytes
                .as_ref()
                .is_ok_and(|bytes| crate::tools::image_type(bytes).is_some())
        {
            return;
        }
        let entry = match bytes {
            Ok(bytes) => match preview(&bytes) {
                Ok(source) => Entry::Image {
                    source,
                    bytes: bytes.into(),
                    label: path.to_string(),
                },
                Err(error) => Entry::Error(format!("{path}: {error}")),
            },
            Err(error) => Entry::Error(format!("{path}: {error}")),
        };
        self.entries.push(entry);
    }

    pub(in crate::tui::sigil_view) fn place_assistant(&mut self, text: String) {
        let pending = self
            .entries
            .iter_mut()
            .rev()
            .take_while(|entry| {
                matches!(
                    entry,
                    Entry::Assistant { done: false, .. }
                        | Entry::Verify { .. }
                        | Entry::Receipt(_)
                        | Entry::System(_)
                )
            })
            .find_map(|entry| match entry {
                Entry::Assistant { text, done } if !*done => Some((text, done)),
                _ => None,
            });
        if let Some((body, done)) = pending {
            *body = text;
            *done = true;
        } else {
            self.entries.push(Entry::Assistant { text, done: true });
        }
    }

    pub(in crate::tui::sigil_view) fn transcript(&self) -> String {
        let hidden = self.prefs.reasoning == crate::prefs::Visibility::Off;
        self.entries
            .iter()
            .filter(|entry| !(hidden && matches!(entry, Entry::Thinking(_))))
            .map(entry_markdown)
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}
