use super::*;

impl Worker {
    pub(in crate::tui::worker) fn network(&mut self, argument: Option<&str>) -> bool {
        match argument.map(str::trim) {
            None | Some("") => {
                let state = if !self.loop_.has_sandbox() {
                    "sandbox off — commands already have network"
                } else if self.loop_.denies_network() {
                    "network denied in the sandbox — /permissions net on, or net once"
                } else {
                    "network allowed for this session — /permissions net off denies it again"
                };
                self.system(state);
            }
            Some("on") => match self.loop_.set_network(crate::Network::Allowed) {
                Ok(()) => self.system("network allowed for this session"),
                Err(error) => self.fail(&error.to_string()),
            },
            Some("off") => match self.loop_.set_network(crate::Network::Denied) {
                Ok(()) => self.system("network denied for this session"),
                Err(error) => self.fail(&error.to_string()),
            },
            Some("once") => {
                self.loop_.allow_network_once();
                self.system("network allowed for the next command");
            }
            Some(_) => self.system("/permissions net on | off | once"),
        }
        true
    }

    pub(in crate::tui::worker) async fn login(&mut self, name: &str) -> bool {
        let endpoint = match crate::endpoint::login_seat(name) {
            Ok(endpoint) => endpoint,
            Err(error) => {
                self.done(name, Err(error.to_string()));
                return true;
            }
        };
        let notice = self.ui.clone();
        let asking = self.ui.clone();
        let (answers, typed) = tokio::sync::mpsc::unbounded_channel::<String>();
        let typed = std::sync::Arc::new(tokio::sync::Mutex::new(typed));
        let mut prompt = summonr_auth::Prompt::new(
            move |line| drop(notice.send(step(line, false))),
            move |line| {
                let asking = asking.clone();
                let typed = typed.clone();
                let step = step(&line, true);
                Box::pin(async move {
                    drop(asking.send(step));
                    typed.lock().await.recv().await
                })
            },
        );
        let login = crate::provider::Client::login(&endpoint, &mut prompt);
        drop(self.ui.send(Msg::Busy(Some(Activity::Login))));
        let outcome = attend(self.inbox(Some(answers)), login).await;
        let running = match outcome {
            Attended::Done(Ok(())) => {
                if name == self.endpoint {
                    self.set_endpoint(name).await;
                }
                self.done(name, Ok(format!("✓ Signed in to {name}.")));
                true
            }
            Attended::Done(Err(error)) => {
                self.done(name, Err(format!("✗ {error}. ↵ try again")));
                self.recover();
                true
            }
            Attended::Cancelled => {
                self.done(name, Err(String::new()));
                self.recover();
                true
            }
            Attended::Quit => false,
        };
        drop(self.ui.send(Msg::Busy(None)));
        running
    }

    pub(in crate::tui::worker) async fn set_key(&mut self, name: &str, key: &str) -> bool {
        let outcome = attend(self.inbox(None), checked(name, key)).await;
        let (result, rejected) = match outcome {
            Attended::Done(Ok(text)) => (Ok(text), false),
            Attended::Done(Err((text, rejected))) => (Err(text), rejected),
            Attended::Cancelled => (Err(String::new()), false),
            Attended::Quit => return false,
        };
        if result.is_ok() && name == self.endpoint {
            self.set_endpoint(name).await;
        }
        self.report(name, result, rejected);
        true
    }

    pub(in crate::tui::worker) async fn logout(&mut self, name: &str) {
        let result = crate::endpoint::logout(name).map_err(|error| error.to_string());
        if result.is_ok() && name == self.endpoint {
            self.set_endpoint(name).await;
        }
        self.done(name, result);
    }

    fn done(&self, name: &str, result: std::result::Result<String, String>) {
        self.report(name, result, false);
    }

    fn report(&self, name: &str, result: std::result::Result<String, String>, rejected: bool) {
        drop(self.ui.send(Msg::LoginDone {
            name: name.to_string(),
            result,
            rejected,
        }));
    }

    fn inbox(&mut self, answers: Option<tokio::sync::mpsc::UnboundedSender<String>>) -> Inbox<'_> {
        Inbox {
            cmd: &mut self.cmd,
            reader: &self.reader,
            followups: &mut self.followups,
            deferred: &mut self.deferred,
            ui: &self.ui,
            answers,
        }
    }

    pub(in crate::tui::worker) fn permissions(&mut self, argument: Option<&str>) -> bool {
        let (topic, rest) = argument.map_or(("", None), |text| {
            text.split_once(char::is_whitespace)
                .map_or((text, None), |(topic, rest)| (topic, Some(rest.trim())))
        });
        match topic {
            "" => self.network(None) && self.write(None),
            "net" => self.network(rest),
            "write" => self.write(rest),
            other => {
                self.fail(&format!("/permissions takes net or write, not `{other}`"));
                true
            }
        }
    }
}

fn step(line: &str, asking: bool) -> Msg {
    let rest = line.strip_prefix("open ").unwrap_or_default();
    let (url, code) = rest.split_once(" and enter ").unwrap_or((rest, ""));
    Msg::LoginStep {
        text: if rest.is_empty() {
            line.to_string()
        } else {
            String::new()
        },
        url: url.to_string(),
        code: code.to_string(),
        asking,
    }
}

async fn checked(name: &str, key: &str) -> std::result::Result<String, (String, bool)> {
    let source = crate::endpoint::check_key(name, key)
        .await
        .map_err(|error| match error.provider_failure() {
            Some(failure) => (
                format!("✗ {failure}"),
                failure.kind == crate::error::ProviderFailureKind::Authentication,
            ),
            None => (format!("✗ {error}"), false),
        })?;
    let store = match source {
        crate::secret::Source::Keychain => "the keychain",
        crate::secret::Source::File => "credentials.toml",
        crate::secret::Source::Config => "config.toml",
        crate::secret::Source::Env
        | crate::secret::Source::Builtin
        | crate::secret::Source::Peer(_) => source.label(),
    };
    Ok(format!("✓ Saved the {name} key in {store}."))
}
