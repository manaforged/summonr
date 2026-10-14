use super::entry::linked;
use super::*;

impl Model {
    pub(in crate::tui::sigil_view) fn start_tool(
        &mut self,
        id: String,
        name: String,
        arguments: String,
    ) {
        if polls_child(&name, &arguments) {
            self.live_tool = Some((name, String::new()));
            return;
        }
        let arg = tool_arg(&arguments);
        if self.group(&id, &name, arg.clone()) {
            self.live_tool = Some((name, arg));
            return;
        }
        self.tools.insert(id.clone(), self.entries.len());
        self.entries.push(Entry::Tool {
            id,
            name: name.clone(),
            arg: arg.clone(),
            state: ToolState::Running,
            output: String::new(),
            expanded: false,
            patch: None,
        });
        self.live_tool = Some((name, arg));
    }

    fn group(&mut self, id: &str, name: &str, arg: String) -> bool {
        if crate::tools::Builtin::of(name)
            .is_some_and(|kind| kind.edits() || kind == crate::tools::Builtin::Task)
        {
            return false;
        }
        enum Tail {
            Group,
            Single,
        }
        let last = self.entries.last();
        let tail = if matches!(last, Some(Entry::ToolGroup { name: prior, .. }) if prior == name) {
            Tail::Group
        } else if matches!(
            last,
            Some(Entry::Tool {
                name: prior,
                state: ToolState::Ok | ToolState::Failed,
                ..
            }) if prior == name
        ) {
            Tail::Single
        } else {
            return false;
        };
        let index = self.entries.len().saturating_sub(1);
        match tail {
            Tail::Group => {
                let Some(Entry::ToolGroup { calls, .. }) = self.entries.last_mut() else {
                    return false;
                };
                calls.push(Call::new(
                    id.to_string(),
                    arg,
                    ToolState::Running,
                    String::new(),
                ));
            }
            Tail::Single => {
                let Some(Entry::Tool {
                    id: old_id,
                    arg: old_arg,
                    state: old_state,
                    output: old_output,
                    expanded,
                    patch,
                    ..
                }) = self.entries.inner.pop()
                else {
                    return false;
                };
                let calls = vec![
                    Call {
                        id: old_id,
                        arg: old_arg,
                        state: old_state,
                        output: old_output,
                        expanded,
                        patch,
                    },
                    Call::new(id.to_string(), arg, ToolState::Running, String::new()),
                ];
                self.entries.push(Entry::ToolGroup {
                    name: name.to_string(),
                    calls,
                });
            }
        }
        self.tools.insert(id.to_string(), index);
        true
    }

    pub(in crate::tui::sigil_view) fn finish_tool(
        &mut self,
        id: &str,
        success: bool,
        output: String,
        patch: Option<String>,
    ) {
        let state = if success {
            ToolState::Ok
        } else {
            ToolState::Failed
        };
        if let Some(index) = self.tools.remove(id) {
            if let Some(Entry::Tool {
                state: stored,
                output: body,
                patch: shown,
                ..
            }) = self.entries.get_mut(index)
            {
                *stored = state;
                *body = output;
                *shown = patch;
            } else if let Some(Entry::ToolGroup { calls, .. }) = self.entries.get_mut(index)
                && let Some(call) = calls.iter_mut().find(|call| call.id == id)
            {
                call.state = state;
                call.output = output;
                call.patch = patch;
            }
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

fn polls_child(name: &str, arguments: &str) -> bool {
    crate::tools::Builtin::is(name, crate::tools::Builtin::Task)
        && serde_json::from_str::<serde_json::Value>(arguments).is_ok_and(|value| {
            value.get("child").is_some()
                && value.get("kill").and_then(serde_json::Value::as_bool) != Some(true)
        })
}
