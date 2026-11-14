use super::*;

impl Worker {
    pub(in crate::hub::worker) async fn login(&mut self, name: &str) -> bool {
        let endpoint = match crate::endpoint::login_endpoint(name) {
            Ok(endpoint) => endpoint,
            Err(error) => {
                self.done(name, Err(error.to_string()));
                return true;
            }
        };
        let notice = self.ui.clone();
        let linked = self.ui.clone();
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
        )
        .with_link(move |url, code| {
            drop(linked.send(Msg::Fact(Fact::LoginStep {
                text: String::new(),
                url: url.to_string(),
                code: code.to_string(),
                asking: false,
            })));
        });
        let login = crate::provider::Client::login(&endpoint, &mut prompt);
        drop(self.ui.send(Msg::Fact(Fact::Busy(Some(Activity::Login)))));
        let outcome = self.wait(Some(answers), login).await;
        let running = match outcome {
            Completion::Done(Ok(())) => {
                if name == self.endpoint {
                    self.selection(Some(name), None).await;
                }
                self.done(name, Ok(format!("✓ Signed in to {name}.")));
                true
            }
            Completion::Done(Err(error)) => {
                self.done(name, Err(format!("✗ {error}. ↵ try again")));
                self.recover();
                true
            }
            Completion::Cancelled => {
                self.done(name, Err(String::new()));
                self.recover();
                true
            }
            Completion::Quit => false,
        };
        drop(self.ui.send(Msg::Fact(Fact::Busy(None))));
        running
    }

    pub(in crate::hub::worker) async fn set_key(&mut self, name: &str, key: &str) -> bool {
        let outcome = self.wait(None, checked(name, key)).await;
        let (result, rejected) = match outcome {
            Completion::Done(Ok(text)) => (Ok(text), false),
            Completion::Done(Err((text, rejected))) => (Err(text), rejected),
            Completion::Cancelled => (Err(String::new()), false),
            Completion::Quit => return false,
        };
        if result.is_ok() && name == self.endpoint {
            self.selection(Some(name), None).await;
        }
        self.report(name, result, rejected);
        true
    }

    pub(in crate::hub::worker) async fn logout(&mut self, name: &str) {
        let result = crate::endpoint::logout(name).map_err(|error| error.to_string());
        if result.is_ok() && name == self.endpoint {
            self.selection(Some(name), None).await;
        }
        self.done(name, result);
    }

    fn done(&self, name: &str, result: std::result::Result<String, String>) {
        self.report(name, result, false);
    }

    fn report(&self, name: &str, result: std::result::Result<String, String>, rejected: bool) {
        drop(self.ui.send(Msg::Fact(Fact::LoginDone {
            name: name.to_string(),
            result,
            rejected,
        })));
    }

    async fn wait<T>(
        &mut self,
        answers: Option<UnboundedSender<String>>,
        task: impl Future<Output = T>,
    ) -> Completion<T> {
        wait_cancelable(
            &mut self.cmd,
            &mut self.queue,
            &self.reader,
            &self.ui,
            Waiting::Login,
            answers,
            task,
        )
        .await
    }

    pub(in crate::hub::worker) fn permissions(&mut self, argument: Option<&str>) -> bool {
        let (topic, rest) = argument.map_or(("", None), |text| {
            text.split_once(char::is_whitespace)
                .map_or((text, None), |(topic, rest)| (topic, Some(rest.trim())))
        });
        match topic {
            "" => self.write(None),
            "write" => self.write(rest),
            other => {
                self.fail(&format!("/permissions takes write, not `{other}`"));
                true
            }
        }
    }
}

fn step(line: &str, asking: bool) -> Msg {
    Msg::Fact(Fact::LoginStep {
        text: line.to_string(),
        url: String::new(),
        code: String::new(),
        asking,
    })
}

async fn checked(name: &str, key: &str) -> std::result::Result<String, (String, bool)> {
    let source = crate::endpoint::check_key(name, key)
        .await
        .map_err(|error| match error.provider_failure() {
            Some(failure) => (
                format!("✗ {failure}"),
                matches!(
                    failure.kind(),
                    summonr_core::error::ProviderFailureKind::Authentication
                        | summonr_core::error::ProviderFailureKind::MissingKey
                ),
            ),
            None => (format!("✗ {error}"), false),
        })?;
    let store = match source {
        crate::secret::Source::Keychain => "the keychain",
        crate::secret::Source::File => "credentials.toml",
        crate::secret::Source::Config => crate::config::NAME,
        crate::secret::Source::Env | crate::secret::Source::Builtin => source.label(),
    };
    Ok(format!("✓ Saved the {name} key in {store}."))
}
