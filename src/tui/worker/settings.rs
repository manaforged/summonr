use super::Worker;
use crate::provider::Client;
use crate::runtime::Loop;
use crate::tui::{Msg, Snapshot};
use tokio::sync::mpsc::UnboundedSender;

impl Worker {
    pub(super) fn list_models(&self) {
        let models: Vec<String> = crate::endpoint::choices()
            .into_iter()
            .map(|(name, model)| format!("{name}\t{model}"))
            .collect();
        if models.is_empty() {
            drop(
                self.ui
                    .send(Msg::ModelsFailed("no resolvable endpoints".to_string())),
            );
        } else {
            drop(self.ui.send(Msg::Models(models)));
        }
    }

    pub(super) fn set_verify(&mut self, on: bool) {
        if on {
            match &self.check {
                Some(commands) => self.loop_.set_verify(Some(commands.clone())),
                None => self.fail("no verify check configured — add one to .mage/check.toml"),
            }
        } else {
            self.loop_.set_verify(None);
        }
        self.send_settings();
    }

    pub(super) fn set_endpoint(&mut self, name: &str) {
        let resolved = crate::endpoint::resolve(Some(name), None)
            .and_then(|endpoint| Client::connect(&endpoint).map(|provider| (endpoint, provider)));
        match resolved {
            Ok((endpoint, provider)) => self.use_endpoint(endpoint, provider),
            Err(error) => self.fail(&error.to_string()),
        }
        self.send_settings();
    }

    pub(super) fn set_model(&mut self, model: &str) {
        let resolved = crate::endpoint::resolve(Some(&self.endpoint), Some(model))
            .and_then(|endpoint| Client::connect(&endpoint).map(|provider| (endpoint, provider)));
        match resolved {
            Ok((endpoint, provider)) => self.use_endpoint(endpoint, provider),
            Err(error) => self.fail(&error.to_string()),
        }
        self.send_settings();
    }

    fn use_endpoint(&mut self, endpoint: crate::endpoint::Endpoint, provider: Client) {
        let delta = self.ui.clone();
        let thinking = self.ui.clone();
        let provider = provider
            .on_delta(move |text| drop(delta.send(Msg::Delta(text.to_string()))))
            .on_reasoning(move |text| {
                drop(thinking.send(Msg::Thinking(text.to_string())));
            });
        self.relay = crate::splash::host_of(endpoint.base_url.as_deref()).map(str::to_string);
        self.endpoint = endpoint.name;
        self.loop_.set_context_window(endpoint.context_window);
        self.loop_.set_env(endpoint.env);
        self.loop_.set_provider(provider, endpoint.model);
        drop(self.ui.send(Msg::Model {
            model: self.loop_.model().to_string(),
            relay: self.relay.clone(),
        }));
    }

    pub(super) fn send_settings(&self) {
        drop(self.ui.send(Msg::Settings(Snapshot {
            model: self.loop_.model().to_string(),
            relay: self.relay.clone(),
            plan: self.loop_.is_plan(),
            verify: verify_display(self.loop_.verify_commands()),
            max_rounds: self.loop_.max_rounds(),
            strict: self.loop_.is_strict(),
            endpoint: self.endpoint.clone(),
            context_window: self.loop_.context_window(),
        })));
    }
}

pub(super) fn set_mode(loop_: &mut Loop<Client>, ui: &UnboundedSender<Msg>, plan: bool) {
    if loop_.is_plan() == plan {
        return;
    }
    loop_.set_plan(plan);
    drop(ui.send(Msg::Mode(plan)));
    let text = if plan {
        "plan mode — read-only; the plan is published into the session journal"
    } else {
        "auto mode — full tools"
    };
    drop(ui.send(Msg::System(text.to_string())));
}

/// The modal shows one verify row; several gates collapse to the first
/// plus a count.
fn verify_display(commands: &[String]) -> Option<String> {
    match commands {
        [] => None,
        [one] => Some(one.clone()),
        [first, rest @ ..] => Some(format!("{first} (+{} more)", rest.len())),
    }
}
