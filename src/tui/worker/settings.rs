use super::Worker;
use crate::endpoint::Endpoint;
use crate::provider::Client;
use crate::runtime::Loop;
use crate::tui::{Msg, Snapshot};
use crate::workflow::{self, ModelSelection, Role, Workflow};
use tokio::sync::mpsc::UnboundedSender;

const WORKFLOW_USAGE: &str = "/workflow [NAME | off | list | show | set ROLE ENDPOINT [MODEL] | \
clear ROLE | save NAME | default NAME]; roles: scout implement review judge summary";

impl Worker {
    pub(super) fn set_verify(&mut self, on: bool) {
        if on {
            match crate::verify::check_commands(self.loop_.workspace()) {
                Ok(Some(commands)) => {
                    if let Err(error) = self.loop_.set_verify(Some(commands)) {
                        self.fail(&error.to_string());
                    }
                }
                Ok(None) => {
                    self.fail("Add a check to .summonr/check.toml, then enable Verify again.")
                }
                Err(error) => self.fail(&error.to_string()),
            }
        } else {
            drop(self.loop_.set_verify(None));
        }
        self.send_settings();
    }

    pub(super) async fn selection(&mut self, endpoint: Option<&str>, model: Option<&str>) {
        let endpoint = crate::endpoint::resolve(
            Some(endpoint.unwrap_or(&self.endpoint)),
            model,
            crate::endpoint::Need::ModelAndKey,
        );
        self.drive(endpoint).await;
    }

    pub(super) async fn drive(&mut self, endpoint: crate::Result<Endpoint>) {
        match endpoint
            .and_then(|endpoint| Self::client(&self.ui, &endpoint).map(|client| (endpoint, client)))
        {
            Ok((endpoint, provider)) => {
                let probe = endpoint
                    .context_window
                    .is_none()
                    .then(|| (provider.clone(), endpoint.model.clone()));
                self.use_endpoint(endpoint, provider);
                self.remember_model();
                self.send_settings();
                if let Some((provider, model)) = probe {
                    self.loop_
                        .set_context_window(provider.context(&self.endpoint, &model).await);
                    self.send_settings();
                }
            }
            Err(error) => {
                self.fail(&error.to_string());
                self.send_settings();
            }
        }
    }

    fn client(ui: &UnboundedSender<Msg>, endpoint: &Endpoint) -> crate::Result<Client> {
        crate::tui::channel::client(ui, endpoint)
    }

    pub(super) fn workflow(&mut self, argument: Option<&str>) {
        let words: Vec<&str> = argument
            .map(|argument| argument.split_whitespace().collect())
            .unwrap_or_default();
        match words.as_slice() {
            [] | ["list"] => self.list_workflows(),
            ["show"] => self.show_workflow(),
            ["off"] => self.clear_workflow(),
            ["set", role, endpoint] => self.set_role(role, endpoint, None),
            ["set", role, endpoint, model] => self.set_role(role, endpoint, Some(model)),
            ["clear", role] => match role.parse() {
                Ok(role) => self.clear_role(role),
                Err(error) => self.fail(&error.to_string()),
            },
            ["save", name] => self.save_workflow(name),
            ["default", name] => match workflow::set_default(name) {
                Ok(()) => self.system(&format!("workflow {name} is the default for new sessions")),
                Err(error) => self.fail(&error.to_string()),
            },
            [name] => self.apply_workflow(name),
            _ => self.system(WORKFLOW_USAGE),
        }
    }

    fn show_workflow(&self) {
        let name = self
            .loop_
            .workflow()
            .unwrap_or_else(|| "(unsaved)".to_string());
        self.system(&format!("workflow {name} · {}", self.workflow.summary()));
    }

    fn clear_workflow(&mut self) {
        for role in Role::ALL {
            self.loop_.clear_role_model(role);
        }
        self.workflow = Workflow::default();
        match self.loop_.set_workflow(None) {
            Ok(()) => self.system("workflow off: every role on the primary model"),
            Err(error) => self.fail(&error.to_string()),
        }
        self.send_workflow();
        self.send_settings();
    }

    fn set_role(&mut self, role: &str, endpoint: &str, model: Option<&str>) {
        match role.parse() {
            Ok(role) => self.assign_model(role, endpoint, model),
            Err(error) => self.fail(&error.to_string()),
        }
    }

    pub(super) fn assign_model(&mut self, role: Role, endpoint: &str, model: Option<&str>) {
        let resolved =
            crate::endpoint::resolve(Some(endpoint), model, crate::endpoint::Need::ModelAndKey)
                .and_then(|endpoint| {
                    Self::client(&self.ui, &endpoint).map(|client| (endpoint, client))
                });
        match resolved {
            Ok((endpoint, client)) => {
                if endpoint.context_window.is_none() {
                    self.loop_.watch_context(
                        Some(role),
                        client.clone(),
                        &endpoint.name,
                        &endpoint.model,
                    );
                }
                self.loop_.set_role_model(
                    role,
                    client,
                    endpoint.model.clone(),
                    endpoint.effort,
                    endpoint.price,
                    endpoint.context_window,
                );
                self.workflow.selections.insert(
                    role,
                    ModelSelection {
                        endpoint: endpoint.name,
                        model: Some(endpoint.model),
                    },
                );
            }
            Err(error) => self.fail(&format!("{role} selection: {error}")),
        }
        self.send_workflow();
    }

    pub(super) fn clear_role(&mut self, role: Role) {
        self.loop_.clear_role_model(role);
        self.workflow.selections.remove(&role);
        self.send_workflow();
    }

    pub(super) fn set_module(&mut self, id: crate::modules::ModuleId, on: bool) {
        if let Err(error) = crate::modules::set(id, on) {
            self.fail(&error.to_string());
            return;
        }
        match crate::modules::load(self.loop_.workspace()) {
            Ok(switches) => {
                if self.loop_.catalog_frozen() {
                    self.system("saved for the next process; this session's tools stay frozen");
                } else if let Err(error) = self.loop_.set_modules(&switches) {
                    self.fail(&error.to_string());
                }
            }
            Err(error) => self.fail(&error.to_string()),
        }
    }

    pub(super) fn save_workflow(&mut self, name: &str) {
        self.workflow.name = name.trim().to_string();
        let saved = workflow::save(&self.workflow)
            .and_then(|()| self.loop_.set_workflow(Some(&self.workflow.name)));
        match saved {
            Ok(()) => self.system(&format!(
                "workflow {} saved · {}",
                self.workflow.name,
                self.workflow.summary()
            )),
            Err(error) => self.fail(&error.to_string()),
        }
        self.send_workflow();
        self.send_settings();
    }

    pub(super) fn send_workflow(&self) {
        let selections = Role::ALL
            .iter()
            .map(|role| {
                self.workflow.selection(*role).map(|selection| {
                    format!(
                        "{}\t{}",
                        selection.endpoint,
                        selection.model.as_deref().unwrap_or_default()
                    )
                })
            })
            .collect();
        drop(self.ui.send(Msg::Workflow {
            name: self.loop_.workflow(),
            names: workflow::names().unwrap_or_default(),
            selections,
        }));
    }

    fn list_workflows(&self) {
        let active = self.loop_.workflow();
        match workflow::names() {
            Ok(names) if names.is_empty() => {
                self.system("no workflows; add [workflows.NAME] to ~/.summonr/config.toml");
            }
            Ok(names) => {
                let rows = names
                    .iter()
                    .map(|name| {
                        let mark = if active.as_deref() == Some(name.as_str()) {
                            "› "
                        } else {
                            "  "
                        };
                        let selections = workflow::load(name)
                            .map(|workflow| workflow.summary())
                            .unwrap_or_else(|error| error.to_string());
                        format!("{mark}{name} · {selections}")
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                self.system(&format!(
                    "workflows (/workflow NAME · /workflow off)\n{rows}"
                ));
            }
            Err(error) => self.fail(&error.to_string()),
        }
    }

    pub(super) fn apply_workflow(&mut self, name: &str) {
        let workflow = match workflow::load(name) {
            Ok(workflow) => workflow,
            Err(error) => return self.fail(&error.to_string()),
        };
        let ui = &self.ui;
        let selected = self.loop_.configure_workflow(&workflow, |selection| {
            let endpoint = crate::endpoint::resolve(
                Some(&selection.endpoint),
                selection.model.as_deref(),
                crate::endpoint::Need::ModelAndKey,
            )?;
            Ok((Self::client(ui, &endpoint)?, endpoint))
        });
        if let Err(error) = selected {
            return self.fail(&error.to_string());
        }
        self.system(&format!("workflow {name} · {}", workflow.summary()));
        self.workflow = workflow;
        self.send_workflow();
        self.send_settings();
    }

    fn use_endpoint(&mut self, endpoint: Endpoint, provider: Client) {
        let (fallbacks, skipped) =
            crate::endpoint::fallbacks(&endpoint, |fallback| Self::client(&self.ui, fallback));
        for note in skipped {
            self.fail(&note);
        }
        self.loop_.set_fallbacks(fallbacks);
        self.relay = crate::splash::host_of(endpoint.base_url.as_deref()).map(str::to_string);
        self.endpoint = endpoint.name.clone();
        self.levels = endpoint.reasoning.levels_for(&endpoint.model);
        self.loop_.choose_effort(self.dial);
        self.loop_.apply_endpoint(&endpoint);
        let effort = self.loop_.effort().map(|effort| effort.clamp(&self.levels));
        self.loop_.set_effort(effort);
        self.loop_.set_provider(provider, endpoint.model);
    }

    pub(super) fn remember_model(&self) {
        if let Some(home) = &self.home {
            let selection = crate::recent::ModelId::new(
                crate::endpoint::remembered(&self.endpoint),
                self.loop_.model(),
            );
            drop(crate::recent::record(home, &selection, self.loop_.effort()));
        }
    }

    pub(super) fn send_settings(&self) {
        drop(
            self.ui.send(Msg::Settings(Snapshot {
                model: self.loop_.model().to_string(),
                relay: self.relay.clone(),
                work: self.loop_.is_work(),
                goal: self.loop_.is_goal(),
                effort: self.loop_.effort(),
                levels: self.levels.clone(),
                verify: verify_display(self.loop_.verify_commands()),
                strict: self.loop_.is_strict(),
                approval: self.loop_.is_approval(),
                endpoint: self.endpoint.clone(),
                context_window: self.loop_.pressure_window(),
                prompt_system: self.loop_.prompt_parts().0,
                prompt_schemas: self.loop_.prompt_parts().1,
                prompt_tools: self.loop_.prompt_parts().2,
                prompt_other: self.loop_.prompt_parts().3,
                workflow: self.loop_.workflow(),
                unconfigured: crate::endpoint::resolve(
                    Some(&self.endpoint),
                    None,
                    crate::endpoint::Need::ModelAndKey,
                )
                .is_err(),
            })),
        );
    }
}

pub(super) fn set_run_mode(
    loop_: &mut Loop<Client>,
    ui: &UnboundedSender<Msg>,
    mode: crate::WorkKind,
) {
    if loop_.mode() == mode {
        return;
    }
    loop_.set_mode(mode);
    announce_run_mode(loop_, ui);
}

pub(super) fn announce_run_mode(loop_: &Loop<Client>, ui: &UnboundedSender<Msg>) {
    let mode = loop_.mode();
    drop(ui.send(Msg::RunMode(mode)));
    let text = match mode {
        crate::WorkKind::Steps => "work mode: run the steps in order with a check per step",
        crate::WorkKind::Goal => "goal mode: keep going until the objective is met",
        crate::WorkKind::Single => "auto mode: full tools",
    };
    drop(ui.send(Msg::System(text.to_string())));
}

fn verify_display(commands: &[String]) -> Option<String> {
    match commands {
        [] => None,
        [one] => Some(one.clone()),
        [first, rest @ ..] => Some(format!("{first} (+{} more)", rest.len())),
    }
}
