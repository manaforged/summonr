use super::*;

impl Worker {
    pub(in crate::tui::worker) fn session(&self) -> bool {
        self.system(&format!(
            "{}{} · {}\n{} · {}{}",
            self.loop_.session_id(),
            self.loop_
                .name()
                .map_or_else(String::new, |name| format!(" ({name})")),
            self.loop_.session_path().display(),
            self.endpoint,
            self.loop_.model(),
            self.loop_
                .effort()
                .map_or_else(String::new, |effort| format!(
                    " · effort {}",
                    effort.as_str()
                ))
        ));
        true
    }

    pub(in crate::tui::worker) fn resume(&self, key: Option<&str>) -> bool {
        let selected = match key {
            Some(key) => crate::session::resolve_id(key).and_then(|path| {
                crate::session::list()?
                    .into_iter()
                    .find(|info| info.path == path)
                    .ok_or_else(|| crate::Error::Config(format!("session `{key}` disappeared")))
            }),
            None => crate::session::previous_for(
                self.loop_.workspace(),
                &self.loop_.session_id().to_string(),
            )
            .and_then(|info| {
                info.ok_or_else(|| {
                    crate::Error::Config("no previous session for this workspace".to_string())
                })
            }),
        };
        match selected {
            Ok(info) => drop(self.ui.send(Msg::Resume(info.id))),
            Err(error) => self.fail(&error.to_string()),
        }
        true
    }

    pub(in crate::tui::worker) fn name(&self, name: Option<&str>) -> bool {
        match name {
            Some(name) => match self.loop_.set_name(name) {
                Ok(()) => self.system(&format!("session named {name}")),
                Err(error) => self.fail(&error.to_string()),
            },
            None => match self.loop_.name() {
                Some(name) => self.system(&format!("session name: {name}")),
                None => self.system("this session has no name — /name TEXT"),
            },
        }
        true
    }

    pub(in crate::tui::worker) fn undo(&mut self) -> bool {
        match self.loop_.undo() {
            Ok((files, session)) => {
                self.last_turn = None;
                match session {
                    Some(id) => {
                        drop(self.ui.send(Msg::Reset));
                        self.replay();
                        drop(self.ui.send(Msg::Session(id)));
                    }
                    None => drop(self.ui.send(Msg::Cleared)),
                }
                self.send_settings();
                self.system(&match files {
                    Some(paths) if paths.is_empty() => {
                        "Undid the last turn. No files had changed.".to_string()
                    }
                    Some(paths) => format!(
                        "Undid the last turn and restored {} file(s): {}.",
                        paths.len(),
                        paths.join(", ")
                    ),
                    None => "Undid the last turn. Files were not restored: no git checkpoint."
                        .to_string(),
                });
            }
            Err(error) => self.fail(&error.to_string()),
        }
        true
    }

    pub(in crate::tui::worker) fn rollback(&mut self, argument: Option<&str>) -> bool {
        let turn = match argument.map(str::trim).filter(|text| !text.is_empty()) {
            None => None,
            Some(text) => match text.parse::<usize>() {
                Ok(turn) => Some(turn),
                Err(_) => {
                    self.fail("/undo files takes a turn number");
                    return true;
                }
            },
        };
        match self.loop_.rollback(turn) {
            Ok((turn, paths)) if paths.is_empty() => {
                self.system(&format!("Nothing changed since before turn {turn}."));
            }
            Ok((turn, paths)) => self.system(&format!(
                "Restored {} file(s) to before turn {turn}: {}. The conversation is unchanged.",
                paths.len(),
                paths.join(", ")
            )),
            Err(error) => self.fail(&error.to_string()),
        }
        true
    }

    pub(in crate::tui::worker) fn fork(&mut self) -> bool {
        match self.loop_.fork_live() {
            Ok(id) => {
                self.system(&format!(
                    "Forked to session {id}. This branch continues here."
                ));
                drop(self.ui.send(Msg::Session(id)));
            }
            Err(error) => self.fail(&error.to_string()),
        }
        true
    }

    pub(in crate::tui::worker) fn branch(&mut self, session: &str, turn: usize) -> bool {
        if self.loop_.session_id() != session {
            self.fail("The active session changed. Run /fork TEXT again.");
            drop(self.ui.send(Msg::Busy(None)));
            return true;
        }
        match self.loop_.fork_at(Some(turn)) {
            Ok(id) => {
                self.last_turn = None;
                drop(self.ui.send(Msg::Reset));
                self.replay();
                drop(self.ui.send(Msg::Session(id)));
                self.send_settings();
                self.system(&format!(
                    "Branched after turn {}. Workspace files are unchanged. Use /resume to return to the parent.",
                    turn + 1
                ));
            }
            Err(error) => self.fail(&error.to_string()),
        }
        drop(self.ui.send(Msg::Busy(None)));
        true
    }

    pub(in crate::tui::worker) fn take_clear(&mut self, head: &str) -> bool {
        let position = self.deferred.iter().position(|command| {
            matches!(
                command,
                Cmd::Command(line)
                    if line.trim_start().starts_with(head)
                        && matches!(argument(line), Ok(Some("clear" | "stop" | "off" | "cancel")))
            )
        });
        position.is_some_and(|index| self.deferred.remove(index).is_some())
    }
}
