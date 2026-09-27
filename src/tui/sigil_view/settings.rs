pub(super) use super::app::App;
pub(super) use super::canvas;
pub(super) use super::prelude;

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
    Module(usize),
    Theme,
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
    let mode = if snapshot.plan {
        "plan · read-only"
    } else if snapshot.work {
        "work · execute the plan"
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
            "agent",
            Setting::Tools,
            "tools",
            prefs.tools.name().to_string(),
        ),
        (
            "agent",
            Setting::Mail,
            "mail",
            if prefs.mail {
                "mail starts a turn"
            } else {
                "mail waits"
            }
            .to_string(),
        ),
    ];
    let modules = app.modules.peek();
    for (index, module) in crate::modules::ALL.iter().enumerate() {
        rows.push((
            "modules",
            Setting::Module(index),
            module.name,
            on_off(modules.get(module.name).copied().unwrap_or(module.default)).to_string(),
        ));
    }
    rows.extend([
        ("interface", Setting::Theme, "theme", prefs.theme.clone()),
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
    rows.into_iter()
        .map(|(group, setting, label, value)| Row {
            group,
            setting,
            label: label.to_string(),
            value,
        })
        .collect()
}

const fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}
