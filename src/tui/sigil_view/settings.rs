pub(super) use super::app::App;
pub(super) use super::canvas;

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
    Receipts,
    Hotkeys,
}

#[derive(Clone)]
pub(super) struct Row {
    pub(super) group: &'static str,
    pub(super) setting: Setting,
    pub(super) label: String,
    pub(super) value: String,
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
    let mut rows = vec![
        ("session", Setting::Mode, "mode", mode.to_string()),
        (
            "session",
            Setting::Verify,
            "verify",
            snapshot
                .verify
                .as_deref()
                .map_or_else(|| "off".to_string(), |command| format!("on · {command}")),
        ),
        (
            "session",
            Setting::Workflow,
            "workflow",
            snapshot
                .workflow
                .clone()
                .unwrap_or_else(|| "off".to_string()),
        ),
        (
            "model",
            Setting::Endpoint,
            "endpoint",
            snapshot.relay.as_deref().map_or_else(
                || snapshot.endpoint.clone(),
                |relay| format!("{} · {relay}", snapshot.endpoint),
            ),
        ),
        ("model", Setting::Model, "model", snapshot.model.clone()),
        (
            "model",
            Setting::Reasoning,
            "reasoning",
            prefs.reasoning.name().to_string(),
        ),
        (
            "harness",
            Setting::Tools,
            "tools",
            prefs.tools.name().to_string(),
        ),
        (
            "harness",
            Setting::Mail,
            "incoming mail",
            if prefs.mail {
                "starts a turn"
            } else {
                "waits for the next turn"
            }
            .to_string(),
        ),
    ];
    let mut shown: Vec<Row> = rows.drain(..).map(Row::from).collect();
    let modules = app.modules.peek();
    shown.extend(crate::modules::ALL.iter().map(|module| Row {
        group: "modules",
        setting: Setting::Module(module.id),
        label: format!("{} tools", module.id.name()),
        value: on_off(modules.get(&module.id).copied().unwrap_or(module.default)).to_string(),
    }));
    rows.extend([
        ("interface", Setting::Theme, "theme", prefs.theme.clone()),
        (
            "interface",
            Setting::Background,
            "background",
            prefs.background.name().to_string(),
        ),
        (
            "interface",
            Setting::Notify,
            "notify",
            prefs.notify.label().to_string(),
        ),
        (
            "interface",
            Setting::DiffLayout,
            "diff layout",
            prefs.diff_layout.name().to_string(),
        ),
        (
            "interface",
            Setting::Splash,
            "splash",
            on_off(prefs.splash).to_string(),
        ),
        (
            "interface",
            Setting::Motion,
            "motion",
            on_off(prefs.animations).to_string(),
        ),
        (
            "interface",
            Setting::Receipts,
            "receipts",
            prefs.receipts.name().to_string(),
        ),
        (
            "interface",
            Setting::Hotkeys,
            "hotkeys",
            format!("{} bindings", crate::prefs::Hotkey::ALL.len()),
        ),
    ]);
    shown.extend(rows.into_iter().map(Row::from));
    shown
}

impl From<(&'static str, Setting, &str, String)> for Row {
    fn from((group, setting, label, value): (&'static str, Setting, &str, String)) -> Self {
        Self {
            group,
            setting,
            label: label.to_string(),
            value,
        }
    }
}

const fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}
