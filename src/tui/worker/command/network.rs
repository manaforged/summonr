use super::*;

impl Worker {
    pub(in crate::tui::worker) fn network(&mut self, argument: Option<&str>) -> bool {
        match argument.map(str::trim) {
            None | Some("") => {
                let state = if !self.loop_.has_sandbox() {
                    "sandbox off — commands already have network"
                } else if self.loop_.denies_network() {
                    "network denied in the sandbox — /net on for this session or /net once"
                } else {
                    "network allowed for this session — /net off to deny again"
                };
                self.system(state);
            }
            Some("on") => match self.loop_.set_network(true) {
                Ok(()) => self.system("network allowed for this session"),
                Err(error) => self.fail(&error.to_string()),
            },
            Some("off") => match self.loop_.set_network(false) {
                Ok(()) => self.system("network denied for this session"),
                Err(error) => self.fail(&error.to_string()),
            },
            Some("once") => {
                self.loop_.allow_network_once();
                self.system("network allowed for the next command");
            }
            Some(_) => self.system("/net on | off | once"),
        }
        true
    }

    pub(in crate::tui::worker) fn auth(&self, name: &str) {
        match crate::endpoint::auth_info(name) {
            Ok(info) => self.system(&info.status()),
            Err(error) => self.fail(&error.to_string()),
        }
    }

    pub(in crate::tui::worker) async fn login(&mut self, name: &str) -> bool {
        let info = match crate::endpoint::auth_info(name) {
            Ok(info) => info,
            Err(error) => {
                self.fail(&error.to_string());
                return true;
            }
        };
        if !info.device_login {
            self.auth(name);
            return true;
        }
        let endpoint = match crate::endpoint::resolve(Some(name), None) {
            Ok(endpoint) => endpoint,
            Err(error) => {
                self.fail(&error.to_string());
                return true;
            }
        };
        let notice = self.ui.clone();
        let login = crate::provider::Client::login(&endpoint, move |line| {
            drop(notice.send(Msg::System(line.to_string())));
        });
        self.system(&format!("{name}: signing in. Press Escape to cancel."));
        drop(self.ui.send(Msg::Busy(Some(Activity::Login))));
        let outcome = attend(
            Inbox {
                cmd: &mut self.cmd,
                reader: &self.reader,
                followups: &mut self.followups,
                deferred: &mut self.deferred,
                ui: &self.ui,
            },
            login,
        )
        .await;
        let running = match outcome {
            Attended::Done(Ok(())) => {
                self.system(&format!("{name}: login complete"));
                true
            }
            Attended::Done(Err(error)) => {
                self.fail(&format!("{error}. Use /login {name} to try again."));
                self.recover();
                true
            }
            Attended::Cancelled => {
                self.system(&format!(
                    "{name}: sign-in cancelled. Use /login {name} to try again."
                ));
                self.recover();
                true
            }
            Attended::Quit => false,
        };
        drop(self.ui.send(Msg::Busy(None)));
        running
    }

    pub(in crate::tui::worker) fn effort(&mut self, level: Option<&str>) -> bool {
        match level {
            None => {
                let next = self.loop_.effort().unwrap_or(crate::Effort::None).next();
                self.dial = Some(next);
                self.loop_.set_effort(Some(next));
            }
            Some(level) => match crate::Effort::parse(level) {
                Some(effort) => {
                    self.dial = Some(effort);
                    self.loop_.set_effort(Some(effort));
                }
                None => {
                    self.fail(&format!(
                        "effort takes none, low, medium, or high, not `{level}`"
                    ));
                    return true;
                }
            },
        }
        let current = self
            .loop_
            .effort()
            .map_or("endpoint default", crate::Effort::as_str);
        let scope = if self.dial.is_some() {
            " · not saved"
        } else {
            ""
        };
        self.system(&format!("effort: {current}{scope}"));
        self.send_settings();
        true
    }
}
