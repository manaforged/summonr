pub(super) use super::app::App;
pub(super) use super::canvas;

use std::rc::Rc;

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

#[derive(Clone)]
pub(super) struct Row {
    pub(super) group: &'static str,
    pub(super) setting: Setting,
    pub(super) label: String,
    pub(super) value: String,
    pub(super) action: Rc<dyn Fn(&App)>,
}

fn row(
    group: &'static str,
    setting: Setting,
    label: impl Into<String>,
    value: impl Into<String>,
    action: impl Fn(&App) + 'static,
) -> Row {
    Row {
        group,
        setting,
        label: label.into(),
        value: value.into(),
        action: Rc::new(action),
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
    let mut rows = vec![
        row(
            "session",
            Setting::Mode,
            "run mode",
            mode,
            App::cycle_run_mode,
        ),
        row(
            "session",
            Setting::Verify,
            "verify",
            verify,
            App::toggle_verify,
        ),
        row(
            "session",
            Setting::Workflow,
            "workflow",
            workflow,
            App::open_workflow,
        ),
        row(
            "model",
            Setting::Endpoint,
            "endpoint",
            endpoint,
            App::cycle_endpoint,
        ),
        row(
            "model",
            Setting::Model,
            "model",
            snapshot.model.as_str(),
            App::begin_model_edit,
        ),
        row(
            "model",
            Setting::Reasoning,
            "reasoning",
            prefs.reasoning.name(),
            |app| {
                app.set_visibility(|prefs| &mut prefs.reasoning, None);
            },
        ),
        row(
            "harness",
            Setting::Tools,
            "tools",
            prefs.tools.name(),
            App::toggle_tools,
        ),
        row(
            "harness",
            Setting::Mail,
            "incoming mail",
            mail,
            App::toggle_mail,
        ),
    ];
    let modules = app.modules.peek();
    rows.extend(crate::modules::ALL.iter().map(|module| {
        let id = module.id;
        let on = on_off(modules.get(&id).copied().unwrap_or(module.default));
        row(
            "modules",
            Setting::Module(id),
            format!("{} tools", id.name()),
            on,
            move |app| {
                app.toggle_module(id);
            },
        )
    }));
    let hotkeys = format!("{} bindings", crate::prefs::Hotkey::ALL.len());
    rows.extend([
        row(
            "interface",
            Setting::Theme,
            "theme",
            prefs.theme.as_str(),
            App::toggle_theme,
        ),
        row(
            "interface",
            Setting::Background,
            "background",
            prefs.background.name(),
            App::toggle_background,
        ),
        row(
            "interface",
            Setting::Notify,
            "notify",
            prefs.notify.label(),
            |app| {
                app.flip(|prefs| prefs.notify = prefs.notify.toggle());
            },
        ),
        row(
            "interface",
            Setting::DiffLayout,
            "diff layout",
            prefs.diff_layout.name(),
            |app| {
                app.set_diff_layout(app.model.get().prefs.diff_layout.toggle());
            },
        ),
        row(
            "interface",
            Setting::Splash,
            "splash",
            on_off(prefs.splash),
            |app| {
                app.flip(|prefs| prefs.splash = !prefs.splash);
            },
        ),
        row(
            "interface",
            Setting::Motion,
            "motion",
            on_off(prefs.animations),
            App::toggle_motion,
        ),
        row(
            "interface",
            Setting::Hotkeys,
            "hotkeys",
            hotkeys,
            App::open_hotkeys,
        ),
    ]);
    rows
}

const fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}
