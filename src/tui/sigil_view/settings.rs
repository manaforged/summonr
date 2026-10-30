pub(super) use super::app::App;
pub(super) use super::canvas;

use sigil::prelude::{LayoutBuilderExt, LayoutStyle, NodeBuilder, container};

fn dialog_body(children: impl Fn() -> Vec<NodeBuilder> + 'static) -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .gap(canvas::overlay_gap()),
        )
        .children_reactive(children)
}

mod auth;
mod hotkeys;
mod models;
mod options;
mod workflow;

pub(super) use auth::{keys as auth_keys, layer as auth_layer};
pub(super) use hotkeys::{keys as hotkeys_keys, layer as hotkeys_layer};
pub(super) use models::{keys as model_picker_keys, layer as model_picker_layer};
pub(super) use options::{keys as settings_keys, layer as settings_layer};
pub(super) use workflow::{keys as workflow_keys, layer as workflow_layer};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Setting {
    Mode,
    Verify,
    Workflow,
    Endpoint,
    Model,
    Reasoning,
    Tools,
    DiffLayout,
    Mail,
    Module(crate::modules::ModuleId),
    Theme,
    Background,
    Notify,
    Splash,
    Motion,
    Hotkeys,
}

impl Setting {
    pub(super) fn all() -> impl Iterator<Item = Self> {
        [
            Self::Mode,
            Self::Verify,
            Self::Workflow,
            Self::Endpoint,
            Self::Model,
            Self::Reasoning,
            Self::Tools,
            Self::Mail,
        ]
        .into_iter()
        .chain(
            crate::modules::ALL
                .iter()
                .map(|module| Self::Module(module.id)),
        )
        .chain([
            Self::Theme,
            Self::Background,
            Self::Notify,
            Self::DiffLayout,
            Self::Splash,
            Self::Motion,
            Self::Hotkeys,
        ])
    }

    pub(super) fn activate(self, app: &App) {
        match self {
            Self::Mode => app.cycle_run_mode(),
            Self::Verify => app.toggle_verify(),
            Self::Workflow => app.open_workflow(),
            Self::Endpoint => app.cycle_endpoint(),
            Self::Model => app.begin_model_edit(),
            Self::Reasoning => app.set_visibility(|prefs| &mut prefs.reasoning, None),
            Self::Tools => app.toggle_tools(),
            Self::Mail => app.toggle_mail(),
            Self::Module(id) => app.toggle_module(id),
            Self::Theme => app.toggle_theme(),
            Self::Background => app.toggle_background(),
            Self::Notify => app.flip(|prefs| prefs.notify = prefs.notify.toggle()),
            Self::DiffLayout => app.set_diff_layout(app.model.get().prefs.diff_layout.toggle()),
            Self::Splash => app.flip(|prefs| prefs.splash = !prefs.splash),
            Self::Motion => app.toggle_motion(),
            Self::Hotkeys => app.open_hotkeys(),
        }
    }
}

pub(super) struct Row {
    pub(super) group: &'static str,
    pub(super) label: String,
    pub(super) value: String,
}

fn row(group: &'static str, label: impl Into<String>, value: impl Into<String>) -> Row {
    Row {
        group,
        label: label.into(),
        value: value.into(),
    }
}

pub(super) fn setting_rows(app: &App) -> Vec<Row> {
    let model = app.model.get();
    let snapshot = &model.snapshot;
    let prefs = &model.prefs;
    let mode = if snapshot.work {
        "work · run the steps"
    } else if snapshot.goal {
        "goal · loop until met"
    } else {
        "auto"
    };
    let verify = snapshot
        .verify
        .as_deref()
        .map_or_else(|| "off".to_string(), |command| format!("on · {command}"));
    let endpoint = snapshot.relay.as_deref().map_or_else(
        || snapshot.endpoint.clone(),
        |relay| format!("{} · {relay}", snapshot.endpoint),
    );
    let workflow = snapshot.workflow.as_deref().unwrap_or("off");
    let mail = if prefs.mail {
        "starts a turn"
    } else {
        "waits for the next turn"
    };
    let modules = app.modules.get();
    Setting::all()
        .map(|setting| match setting {
            Setting::Mode => row("session", "run mode", mode),
            Setting::Verify => row("session", "verify", &verify),
            Setting::Workflow => row("session", "workflow", workflow),
            Setting::Endpoint => row("model", "endpoint", &endpoint),
            Setting::Model => row("model", "model", &snapshot.model),
            Setting::Reasoning => row("model", "reasoning", prefs.reasoning.name()),
            Setting::Tools => row("harness", "tools", prefs.tools.name()),
            Setting::Mail => row("harness", "incoming mail", mail),
            Setting::Module(id) => row(
                "modules",
                format!("{} tools", id.name()),
                on_off(
                    modules
                        .get(&id)
                        .copied()
                        .unwrap_or(crate::modules::ALL[id as usize].default),
                ),
            ),
            Setting::Theme => row("interface", "theme", &prefs.theme),
            Setting::Background => row("interface", "background", prefs.background.name()),
            Setting::Notify => row("interface", "notify", prefs.notify.label()),
            Setting::DiffLayout => row("interface", "diff layout", prefs.diff_layout.name()),
            Setting::Splash => row("interface", "splash", on_off(prefs.splash)),
            Setting::Motion => row("interface", "motion", on_off(prefs.animations)),
            Setting::Hotkeys => row(
                "interface",
                "hotkeys",
                format!("{} bindings", crate::prefs::Hotkey::ALL.len()),
            ),
        })
        .collect()
}

const fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}
