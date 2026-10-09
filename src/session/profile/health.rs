use std::collections::BTreeMap;

use serde_json::Value;

use crate::tools::Builtin;

use super::fmt::{head_tail, share};

#[derive(Default)]
pub(super) struct Health {
    per_tool: BTreeMap<String, (usize, usize)>,
    native: usize,
    shell: usize,
    commands: usize,
    pipes: usize,
}

impl Health {
    pub(super) fn add(&mut self, name: &str, arguments: &str, success: bool) -> bool {
        let entry = self.per_tool.entry(name.to_string()).or_default();
        entry.0 += 1;
        if !success {
            entry.1 += 1;
        }
        let args: Value = serde_json::from_str(arguments).unwrap_or_default();
        let text = |key: &str| args.get(key).and_then(Value::as_str).unwrap_or_default();
        let kind = Builtin::of(name);
        match kind {
            Some(Builtin::RunCommand) => {
                self.commands += 1;
                let command = text("command");
                if crate::tools::command::searchish(command) {
                    self.shell += 1;
                }
                if head_tail(command) {
                    self.pipes += 1;
                }
            }
            Some(Builtin::Search) => self.native += 1,
            Some(Builtin::Read) => {
                let op = crate::tools::retired_op(name).unwrap_or_else(|| text("op"));
                if op == "search" || (op.is_empty() && !text("pattern").is_empty()) {
                    self.native += 1;
                }
            }
            Some(
                Builtin::Edit
                | Builtin::Write
                | Builtin::Lsp
                | Builtin::Skill
                | Builtin::Todo
                | Builtin::Web
                | Builtin::Task,
            )
            | None => {}
        }
        success && kind.is_some_and(Builtin::edits)
    }

    pub(super) fn lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut tools: Vec<String> = self
            .per_tool
            .iter()
            .map(|(name, (calls, failed))| {
                if *failed > 0 {
                    format!("{name} {calls} ({failed} failed)")
                } else {
                    format!("{name} {calls}")
                }
            })
            .collect();
        tools.sort();
        if !tools.is_empty() {
            out.push(format!("calls    {}", tools.join(" · ")));
        }
        if self.native + self.shell > 0 {
            out.push(format!(
                "search   native {} · shell {}",
                self.native, self.shell
            ));
        }
        if self.commands > 0 {
            out.push(format!(
                "pipes    {} of {} run_command calls pipe to head/tail ({})",
                self.pipes,
                self.commands,
                share(self.pipes as u64, self.commands as u64)
            ));
        }
        out
    }
}
