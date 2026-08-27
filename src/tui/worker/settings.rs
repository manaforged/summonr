use super::Worker;
use crate::endpoint::Endpoint;
use crate::provider::Client;
use crate::runtime::Loop;
use crate::tui::{Msg, Snapshot};
use crate::workflow::{self, Role, Seat, Workflow};
use futures::future::join_all;
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;

const WORKFLOW_USAGE: &str = "/workflow [NAME | off | list | show | set ROLE ENDPOINT [MODEL] | \
clear ROLE | save NAME | default NAME] — roles: scout plan implement review judge summary";

/// How long one endpoint's `/models` probe may run before the picker uses
/// the configured model instead. Discovery runs in the background, so this
/// only bounds how long a slow host delays its enrichment; the picker is
/// already usable from the configured seats.
const PROBE_TIMEOUT: Duration = Duration::from_secs(8);

impl Worker {
    pub(super) fn list_models(&self) {
        let endpoints: Vec<crate::endpoint::Endpoint> = crate::endpoint::ready()
            .into_iter()
            .filter_map(|name| crate::endpoint::resolve(Some(&name), None).ok())
            .collect();
        if endpoints.is_empty() {
            drop(
                self.ui
                    .send(Msg::ModelsFailed("no resolvable endpoints".to_string())),
            );
            return;
        }
        // Recently-used seats that still resolve to a ready endpoint, newest
        // first, so the picker opens on what the user actually uses.
        let ready: std::collections::HashSet<&str> = endpoints
            .iter()
            .map(|endpoint| endpoint.name.as_str())
            .collect();
        let recent: Vec<String> = self
            .home
            .as_deref()
            .map(crate::recent::load)
            .unwrap_or_default()
            .into_iter()
            .filter(|row| {
                row.split('\t')
                    .next()
                    .is_some_and(|name| ready.contains(name))
            })
            .collect();
        // Populate instantly: recents, then each endpoint's configured model,
        // deduped. The picker opens without waiting on any host.
        let seats = endpoints
            .iter()
            .map(|endpoint| format!("{}\t{}", endpoint.name, endpoint.model));
        let rows = merge(recent.clone(), seats);
        drop(self.ui.send(Msg::Models {
            recent: recent.len(),
            rows,
        }));
        // Enrich with each host's live `/models` in the background. A dead
        // host falls back to its configured model and never blocks the worker
        // or freezes the picker.
        let ui = self.ui.clone();
        drop(tokio::spawn(async move {
            let models = discover(endpoints, PROBE_TIMEOUT).await;
            if !models.is_empty() {
                let rows = merge(recent.clone(), models.into_iter());
                drop(ui.send(Msg::Models {
                    recent: recent.len(),
                    rows,
                }));
            }
        }));
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
        self.drive(crate::endpoint::resolve(Some(name), None));
    }

    pub(super) fn set_seat(&mut self, endpoint: &str, model: &str) {
        self.drive(crate::endpoint::resolve(Some(endpoint), Some(model)));
    }

    pub(super) fn set_model(&mut self, model: &str) {
        self.drive(crate::endpoint::resolve(Some(&self.endpoint), Some(model)));
    }

    /// Make a resolved endpoint the driving seat.
    fn drive(&mut self, endpoint: crate::Result<Endpoint>) {
        match endpoint.and_then(|endpoint| self.client(&endpoint).map(|client| (endpoint, client)))
        {
            Ok((endpoint, provider)) => self.use_endpoint(endpoint, provider),
            Err(error) => self.fail(&error.to_string()),
        }
        self.send_settings();
    }

    /// A client for `endpoint` that streams into the transcript.
    fn client(&self, endpoint: &Endpoint) -> crate::Result<Client> {
        let delta = self.ui.clone();
        let thinking = self.ui.clone();
        let calling = self.ui.clone();
        Ok(Client::connect(endpoint)?
            .on_delta(move |text| drop(delta.send(Msg::Delta(text.to_string()))))
            .on_reasoning(move |text| {
                drop(thinking.send(Msg::Thinking(text.to_string())));
            })
            .on_tool(move |name| drop(calling.send(Msg::Calling(name.to_string())))))
    }

    /// The `/workflow` grammar; every card action has a verb here.
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
            Ok(()) => self.system("workflow off — every role on the driving seat"),
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

    /// Seat one role; the change is live at once. Without `model` the
    /// endpoint's configured model is used.
    pub(super) fn seat_role(&mut self, role: Role, endpoint: &str, model: Option<&str>) {
        let resolved = crate::endpoint::resolve(Some(endpoint), model)
            .and_then(|endpoint| self.client(&endpoint).map(|client| (endpoint, client)));
        match resolved {
            Ok((endpoint, client)) => {
                self.loop_.set_seat(role, client, endpoint.model.clone());
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

    /// Write the live seats as `[workflows.NAME]` and name the session's
    /// workflow after it.
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
                self.system("no workflows — add [workflows.NAME] to ~/.mage/config.toml");
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

    /// Seat every role the workflow names; a seat that fails to resolve is
    /// reported and left on the driving seat.
    pub(super) fn apply_workflow(&mut self, name: &str) {
        let workflow = match workflow::load(name) {
            Ok(workflow) => workflow,
            Err(error) => return self.fail(&error.to_string()),
        };
        for role in Role::ALL {
            self.loop_.clear_seat(role);
        }
        for (role, seat) in &workflow.seats {
            let client = crate::endpoint::resolve(Some(&seat.endpoint), seat.model.as_deref())
                .and_then(|endpoint| self.client(&endpoint).map(|client| (endpoint, client)));
            match client {
                Ok((endpoint, client)) => self.loop_.set_seat(*role, client, endpoint.model),
                Err(error) => self.fail(&format!("{name}: {role} seat {seat}: {error}")),
            }
        }
        match self.loop_.set_workflow(Some(name)) {
            Ok(()) => self.system(&format!("workflow {name} · {}", workflow.summary())),
            Err(error) => self.fail(&error.to_string()),
        }
        self.workflow = workflow;
        self.send_workflow();
        self.send_settings();
    }

    fn use_endpoint(&mut self, endpoint: Endpoint, provider: Client) {
        self.relay = crate::splash::host_of(endpoint.base_url.as_deref()).map(str::to_string);
        self.endpoint = endpoint.name;
        self.loop_.set_context_window(endpoint.context_window);
        self.loop_.set_env(endpoint.env);
        self.loop_.set_provider(provider, endpoint.model);
        self.remember_seat();
        drop(self.ui.send(Msg::Model {
            model: self.loop_.model().to_string(),
            relay: self.relay.clone(),
        }));
    }

    /// Record the driving seat as most-recently-used for the picker.
    pub(super) fn remember_seat(&self) {
        if let Some(home) = &self.home {
            let seat = format!("{}\t{}", self.endpoint, self.loop_.model());
            crate::recent::record(home, &seat);
        }
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
            workflow: self.loop_.workflow(),
        })));
    }
}

/// Probe each endpoint's `/models` concurrently and return `name\tmodel`
/// rows. A Completions host that does not answer within [`PROBE_TIMEOUT`]
/// contributes its configured model; native wires use their static catalog.
/// `recent` seats first, then `rest`, dropping duplicates so a recently-used
/// seat appears once at the top.
fn merge(recent: Vec<String>, rest: impl Iterator<Item = String>) -> Vec<String> {
    let mut rows = recent;
    for row in rest {
        if !rows.contains(&row) {
            rows.push(row);
        }
    }
    rows
}

async fn discover(endpoints: Vec<crate::endpoint::Endpoint>, timeout: Duration) -> Vec<String> {
    let catalogs = join_all(endpoints.into_iter().map(|endpoint| async move {
        let name = endpoint.name.clone();
        let fallback = endpoint.model.clone();
        let models = if endpoint.wire == crate::endpoint::Wire::Completions {
            let request = async {
                let client = Client::connect(&endpoint)?;
                client.models().await
            };
            match tokio::time::timeout(timeout, request).await {
                Ok(Ok(models)) => models,
                _ => vec![fallback],
            }
        } else {
            crate::endpoint::catalog(endpoint.wire)
        };
        models
            .into_iter()
            .map(|model| format!("{name}\t{model}"))
            .collect::<Vec<_>>()
    }))
    .await;
    let mut models = Vec::new();
    for model in catalogs.into_iter().flatten() {
        if !models.contains(&model) {
            models.push(model);
        }
    }
    models
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

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
