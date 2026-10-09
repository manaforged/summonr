use super::Worker;
use crate::endpoint::Endpoint;
use crate::provider::Client;
use crate::runtime::Loop;
use crate::tools::Event;
use crate::tui::{Msg, Snapshot};
use crate::workflow::{self, Role, Seat, Workflow};
use tokio::sync::mpsc::UnboundedSender;

const WORKFLOW_USAGE: &str = "/workflow [NAME | off | list | show | set ROLE ENDPOINT [MODEL] | \
clear ROLE | save NAME | default NAME]; roles: scout plan implement review judge summary";

impl Worker {
    pub(super) fn set_verify(&mut self, on: bool) {
        if on {
            match crate::verify::check_commands(self.loop_.workspace()) {
                Ok(Some(commands)) => self.loop_.set_verify(Some(commands)),
                Ok(None) => {
                    self.fail("Add a check to .summonr/check.toml, then enable Verify again.")
                }
                Err(error) => self.fail(&error.to_string()),
            }
        } else {
            self.loop_.set_verify(None);
        }
        self.send_settings();
    }

    pub(super) async fn set_endpoint(&mut self, name: &str) {
        self.drive(crate::endpoint::resolve(
            Some(name),
            None,
            crate::endpoint::Need::Seat,
        ))
        .await;
    }

    pub(super) async fn set_seat(&mut self, endpoint: &str, model: &str) {
        self.drive(crate::endpoint::resolve(
            Some(endpoint),
            Some(model),
            crate::endpoint::Need::Seat,
        ))
        .await;
    }

    pub(super) async fn set_model(&mut self, model: &str) {
        self.drive(crate::endpoint::resolve(
            Some(&self.endpoint),
            Some(model),
            crate::endpoint::Need::Seat,
        ))
        .await;
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
        let delta = ui.clone();
        let thinking = ui.clone();
        let calling = ui.clone();
        Ok(Client::connect(endpoint)?
            .on_delta(move |text| {
                drop(delta.send(Msg::Event(Event::Delta {
                    text: text.to_string(),
                })))
            })
            .on_reasoning(move |text| {
                drop(thinking.send(Msg::Event(Event::Reasoning {
                    text: text.to_string(),
                })));
            })
            .on_tool(move |name| drop(calling.send(Msg::Calling(name.to_string())))))
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
            self.loop_.clear_seat(role);
        }
        self.workflow = Workflow::default();
        match self.loop_.set_workflow(None) {
            Ok(()) => self.system("workflow off: every role on the driving seat"),
            Err(error) => self.fail(&error.to_string()),
        }
        self.send_workflow();
        self.send_settings();
    }

    fn set_role(&mut self, role: &str, endpoint: &str, model: Option<&str>) {
        match role.parse() {
            Ok(role) => self.seat_role(role, endpoint, model),
            Err(error) => self.fail(&error.to_string()),
        }
    }

    pub(super) fn seat_role(&mut self, role: Role, endpoint: &str, model: Option<&str>) {
        let resolved = crate::endpoint::resolve(Some(endpoint), model, crate::endpoint::Need::Seat)
            .and_then(|endpoint| {
                Self::client(&self.ui, &endpoint).map(|client| (endpoint, client))
            });
        match resolved {
            Ok((endpoint, client)) => {
                self.loop_.set_seat(
                    role,
                    client,
                    endpoint.model.clone(),
                    endpoint.effort,
                    endpoint.price,
                );
                self.workflow.seats.insert(
                    role,
                    Seat {
                        endpoint: endpoint.name,
                        model: Some(endpoint.model),
                    },
                );
            }
            Err(error) => self.fail(&format!("{role} seat: {error}")),
        }
        self.send_workflow();
    }

    pub(super) fn clear_role(&mut self, role: Role) {
        self.loop_.clear_seat(role);
        self.workflow.seats.remove(&role);
        self.send_workflow();
    }

    pub(super) fn set_module(&mut self, id: crate::modules::ModuleId, on: bool) {
        if let Err(error) = crate::modules::set(id, on) {
            self.fail(&error.to_string());
            return;
        }
        match crate::modules::Switches::load(self.loop_.workspace()) {
            Ok(switches) => {
                if self.loop_.catalog_frozen() {
                    self.system("saved for the next process; this session's tools stay frozen");
                } else {
                    self.loop_.set_modules(&switches);
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
        let seats = Role::ALL
            .iter()
            .map(|role| {
                self.workflow.seat(*role).map(|seat| {
                    format!(
                        "{}\t{}",
                        seat.endpoint,
                        seat.model.as_deref().unwrap_or_default()
                    )
                })
            })
            .collect();
        drop(self.ui.send(Msg::Workflow {
            name: self.loop_.workflow(),
            names: workflow::names().unwrap_or_default(),
            seats,
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
                        let seats = workflow::load(name)
                            .map(|workflow| workflow.summary())
                            .unwrap_or_else(|error| error.to_string());
                        format!("{mark}{name} · {seats}")
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
        let applied = self.loop_.apply_workflow(&workflow, |seat| {
            let endpoint = crate::endpoint::resolve(
                Some(&seat.endpoint),
                seat.model.as_deref(),
                crate::endpoint::Need::Seat,
            )?;
            let client = Self::client(ui, &endpoint)?;
            Ok((client, endpoint))
        });
        if let Err(error) = applied {
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
        self.endpoint = endpoint.name;
        self.loop_.set_context_window(endpoint.context_window);
        self.loop_.set_compact_tokens(endpoint.compact_tokens);
        self.loop_.set_env(endpoint.env);
        self.levels = endpoint.reasoning.levels_for(&endpoint.model);
        let effort = self
            .dial
            .or(endpoint.effort)
            .or_else(|| crate::recent::default_effort(&self.endpoint, &endpoint.model))
            .map(|effort| effort.clamp(&self.levels));
        self.loop_.set_effort(effort);
        self.loop_.set_price(endpoint.price);
        self.loop_.set_provider(provider, endpoint.model);
    }

    pub(super) fn remember_seat(&self) {
        if let Some(home) = &self.home {
            let seat = crate::recent::Seat::new(self.endpoint.clone(), self.loop_.model());
            drop(crate::recent::record(home, &seat, self.loop_.effort()));
        }
    }

    pub(super) fn send_settings(&self) {
        drop(self.ui.send(Msg::Settings(Snapshot {
            model: self.loop_.model().to_string(),
            relay: self.relay.clone(),
            plan: self.loop_.is_plan(),
            work: self.loop_.is_work(),
            goal: self.loop_.is_goal(),
            effort: self.loop_.effort(),
            levels: self.levels.clone(),
            pending_plan: self.loop_.pending_plan(),
            verify: verify_display(self.loop_.verify_commands()),
            strict: self.loop_.is_strict(),
            endpoint: self.endpoint.clone(),
            context_window: self.loop_.context_window(),
            workflow: self.loop_.workflow(),
        })));
    }
}

pub(super) fn set_run_mode(
    loop_: &mut Loop<Client>,
    ui: &UnboundedSender<Msg>,
    mode: crate::RunMode,
) {
    if loop_.mode() == mode {
        return;
    }
    if loop_.is_plan()
        && mode == crate::RunMode::Auto
        && let Err(error) = loop_.leave_plan()
    {
        drop(ui.send(Msg::System(error.to_string())));
    }
    if let Err(error) = loop_.set_mode(mode) {
        drop(ui.send(Msg::System(format!("sandbox unavailable: {error}"))));
    }
    announce_run_mode(loop_, ui, mode);
}

pub(super) fn announce_run_mode(
    loop_: &Loop<Client>,
    ui: &UnboundedSender<Msg>,
    requested: crate::RunMode,
) {
    let mode = loop_.mode();
    drop(ui.send(Msg::RunMode(mode)));
    let text = if requested == crate::RunMode::Work && mode == crate::RunMode::Plan {
        "Work needs a plan. Plan mode: write one, then /plan accept."
    } else {
        match mode {
            crate::RunMode::Plan => {
                "plan mode: read-only; /plan accept runs the plan, /goal TEXT starts a goal"
            }
            crate::RunMode::Work => {
                "work mode: execute the accepted plan; verify at the end of the turn"
            }
            crate::RunMode::Goal => "goal mode: keep going until the objective is met",
            crate::RunMode::Auto => "auto mode: full tools",
        }
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
