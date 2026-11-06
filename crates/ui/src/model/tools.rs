use super::entry::linked;
use super::*;

impl Model {
    pub(crate) fn entry_at(&self, index: usize) -> (Option<Entry>, bool, Prefs) {
        let live = self.busy_since.is_some() && index + 1 == self.entries.len();
        (self.entries.get(index).cloned(), live, self.prefs.clone())
    }

    pub(crate) fn start_tool(&mut self, id: String, name: String, arguments: String) {
        self.speed.start(Instant::now());
        if polls_child(&name, &arguments) {
            self.doing = Some(Doing::Tool {
                name,
                arg: String::new(),
            });
            return;
        }
        let arg = tool_arg(&arguments);
        if self.group(&id, &name, arg.clone()) {
            self.doing = Some(Doing::Tool { name, arg });
            return;
        }
        self.tools.insert(id.clone(), self.entries.len());
        self.entries.push(Entry::Tool {
            name: name.clone(),
            calls: vec![Call::new(
                id,
                arg.clone(),
                ToolState::Running,
                String::new(),
            )],
        });
        self.doing = Some(Doing::Tool { name, arg });
    }

    fn group(&mut self, id: &str, name: &str, arg: String) -> bool {
        if summonr_agent::tools::Builtin::of(name)
            .is_some_and(|kind| kind.edits() || kind == summonr_agent::tools::Builtin::Task)
        {
            return false;
        }
        let joins = matches!(
            self.entries.last(),
            Some(Entry::Tool { name: prior, calls })
                if prior == name
                    && (calls.len() > 1 || !matches!(calls[0].state, ToolState::Running))
        );
        if !joins {
            return false;
        }
        let index = self.entries.len().saturating_sub(1);
        let Some(Entry::Tool { calls, .. }) = self.entries.last_mut() else {
            return false;
        };
        calls.push(Call::new(
            id.to_string(),
            arg,
            ToolState::Running,
            String::new(),
        ));
        self.tools.insert(id.to_string(), index);
        true
    }

    pub(crate) fn finish_tool(
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
        if let Some(index) = self.tools.remove(id)
            && let Some(Entry::Tool { calls, .. }) = self.entries.get_mut(index)
            && let Some(call) = calls.iter_mut().find(|call| call.id == id)
        {
            call.state = state;
            call.output = output;
            call.patch = patch;
        }
        self.doing = Some(Doing::Thinking);
    }

    pub(crate) fn finish_assistant(&mut self, text: String) {
        let links = linked(&text);
        self.place_assistant(text);
        for path in links {
            self.show(&path);
        }
    }

    pub(crate) fn show_call(&mut self, arguments: &str, quiet: bool) {
        let path = serde_json::from_str::<serde_json::Value>(arguments)
            .ok()
            .and_then(|value| value.get("path")?.as_str().map(str::to_string));
        if let Some(path) = path {
            self.show_path(&path, quiet);
        }
    }

    pub(crate) fn show(&mut self, path: &str) {
        self.show_path(path, false);
    }

    pub(crate) fn show_path(&mut self, path: &str, quiet: bool) {
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
                .is_ok_and(|bytes| summonr_agent::tools::image_type(bytes).is_some())
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

    pub(crate) fn place_assistant(&mut self, text: String) {
        let pending = self
            .entries
            .iter()
            .enumerate()
            .rev()
            .take_while(|(_, entry)| {
                matches!(
                    entry,
                    Entry::Assistant { done: false, .. }
                        | Entry::Verify { .. }
                        | Entry::Receipt(_)
                        | Entry::System(_)
                )
            })
            .find_map(|(at, entry)| {
                matches!(entry, Entry::Assistant { done: false, .. }).then_some(at)
            });
        if let Some(Entry::Assistant { text: body, done }) =
            pending.and_then(|at| self.entries.get_mut(at))
        {
            *body = text;
            *done = true;
        } else {
            self.entries.push(Entry::Assistant { text, done: true });
        }
    }

    pub(crate) fn transcript(&self) -> String {
        let hidden = self.prefs.reasoning == summonr_agent::prefs::Visibility::Off;
        self.entries
            .iter()
            .filter(|entry| !(hidden && matches!(entry, Entry::Thinking(_))))
            .map(entry_markdown)
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

fn polls_child(name: &str, arguments: &str) -> bool {
    summonr_agent::tools::Builtin::is(name, summonr_agent::tools::Builtin::Task)
        && serde_json::from_str::<serde_json::Value>(arguments).is_ok_and(|value| {
            value.get("child").is_some()
                && value.get("kill").and_then(serde_json::Value::as_bool) != Some(true)
        })
}
