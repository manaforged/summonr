pub(super) use super::app::App;
pub(super) use super::canvas;

use sigil::prelude::NodeBuilder;
use sigil::prelude::Widget;

fn dialog_body(children: impl Fn() -> Vec<NodeBuilder> + 'static) -> NodeBuilder {
    sigil::prelude::column()
        .fill_width()
        .gap(canvas::overlay_gap())
        .children_reactive(children)
}

mod auth;
mod hotkeys;
pub(crate) mod models;
mod options;
pub(crate) mod theme;
mod workflow;

pub(super) use auth::{keys as auth_keys, layer as auth_layer};
pub(super) use hotkeys::{keys as hotkeys_keys, layer as hotkeys_layer};
pub(super) use models::{keys as model_picker_keys, layer as model_picker_layer};
pub(super) use options::{keys as settings_keys, layer as settings_layer};
pub(super) use workflow::{keys as workflow_keys, layer as workflow_layer};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Group {
    Models,
    Work,
    Appearance,
    Input,
    Tools,
}

impl Group {
    pub(super) const ALL: [Self; 5] = [
        Self::Models,
        Self::Work,
        Self::Appearance,
        Self::Input,
        Self::Tools,
    ];

    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::Models => "Models & accounts",
            Self::Work => "Work & checks",
            Self::Appearance => "Appearance",
            Self::Input => "Conversation & input",
            Self::Tools => "Tools & integrations",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Setting {
    Mode,
    Verify,
    Workflow,
    Accounts,
    Model,
    Reasoning,
    Tools,
    DiffLayout,
    Mail,
    Browser,
    Module(summonr_agent::modules::ModuleId),
    Theme,
    Background,
    Notify,
    Splash,
    Motion,
    Spinner,
    Hotkeys,
}

impl Setting {
    pub(super) fn all() -> impl Iterator<Item = Self> {
        [
            Self::Mode,
            Self::Verify,
            Self::Workflow,
            Self::Accounts,
            Self::Model,
            Self::Reasoning,
            Self::Tools,
            Self::Mail,
        ]
        .into_iter()
        .chain(
            summonr_agent::modules::ALL
                .iter()
                .map(|module| Self::Module(module.id)),
        )
        .chain([
            Self::Browser,
            Self::Theme,
            Self::Background,
            Self::Notify,
            Self::DiffLayout,
            Self::Splash,
            Self::Motion,
            Self::Spinner,
            Self::Hotkeys,
        ])
    }

    pub(super) const fn group(self) -> Group {
        match self {
            Self::Accounts | Self::Model => Group::Models,
            Self::Mode | Self::Verify | Self::Workflow => Group::Work,
            Self::Theme
            | Self::Background
            | Self::DiffLayout
            | Self::Splash
            | Self::Motion
            | Self::Spinner => Group::Appearance,
            Self::Notify | Self::Mail | Self::Hotkeys | Self::Reasoning | Self::Tools => {
                Group::Input
            }
            Self::Module(_) | Self::Browser => Group::Tools,
        }
    }

    pub(super) const fn opens(self) -> bool {
        matches!(
            self,
            Self::Accounts | Self::Model | Self::Workflow | Self::Theme | Self::Hotkeys
        )
    }

    pub(super) fn activate(self, app: &App) {
        match self {
            Self::Mode => app.cycle_run_mode(),
            Self::Verify => app.toggle_verify(),
            Self::Workflow => app.open_workflow(),
            Self::Accounts => app.begin_auth(None),
            Self::Model => app.begin_model_edit(),
            Self::Reasoning => app.set_visibility(|prefs| &mut prefs.reasoning, None),
            Self::Tools => app.toggle_tools(),
            Self::Mail => app.toggle_mail(),
            Self::Browser => app.choose_chrome(),
            Self::Module(id) => app.toggle_module(id),
            Self::Theme => app.open_theme(),
            Self::Background => app.toggle_background(),
            Self::Notify => app.flip(|prefs| prefs.notify = prefs.notify.toggle()),
            Self::DiffLayout => app.set_diff_layout(app.model.get().prefs.diff_layout.toggle()),
            Self::Splash => app.cycle_splash(),
            Self::Motion => app.toggle_motion(),
            Self::Spinner => app.flip(|prefs| prefs.spinner = prefs.spinner.toggle()),
            Self::Hotkeys => app.open_hotkeys(),
        }
    }
}

pub(super) struct Row {
    pub(super) label: String,
    pub(super) value: String,
}

fn row(label: impl Into<String>, value: impl Into<String>) -> Row {
    Row {
        label: label.into(),
        value: value.into(),
    }
}

pub(super) fn setting_rows(app: &App) -> Vec<Row> {
    let model = app.model.get();
    let snapshot = &model.snapshot;
    let prefs = &model.prefs;
    let mode = match snapshot.mode {
        summonr_agent::WorkKind::Steps => "work · run the steps",
        summonr_agent::WorkKind::Goal => "goal · loop until met",
        summonr_agent::WorkKind::Single => "auto",
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
    let group = app.settings_group.get();
    Setting::all()
        .filter(|setting| group.is_none_or(|group| setting.group() == group))
        .map(|setting| match setting {
            Setting::Mode => row("run mode", mode),
            Setting::Verify => row("verify", &verify),
            Setting::Workflow => row("workflow", workflow),
            Setting::Accounts => row("accounts", &endpoint),
            Setting::Model => row("model", &snapshot.model),
            Setting::Reasoning => row("reasoning display", prefs.reasoning.name()),
            Setting::Tools => row("tool output", prefs.tools.name()),
            Setting::Mail => row("incoming mail", mail),
            Setting::Browser => row(
                "browser",
                summonr_agent::browser::chrome().map_or_else(
                    || "not found · choose".to_string(),
                    |program| summonr_agent::browser::display_name(&program),
                ),
            ),
            Setting::Module(id) => row(
                format!("{} tools", id.name()),
                on_off(
                    modules
                        .get(&id)
                        .copied()
                        .unwrap_or(summonr_agent::modules::ALL[id as usize].default),
                ),
            ),
            Setting::Theme => row("theme", &prefs.theme),
            Setting::Background => row("background", prefs.background.name()),
            Setting::Notify => row("notify", prefs.notify.label()),
            Setting::DiffLayout => row("diff layout", prefs.diff_layout.name()),
            Setting::Splash => row("splash", prefs.splash.name()),
            Setting::Motion => row("motion", on_off(prefs.animations)),
            Setting::Spinner => row("spinner", prefs.spinner.name()),
            Setting::Hotkeys => row(
                "hotkeys",
                format!("{} bindings", summonr_agent::prefs::Hotkey::ALL.len()),
            ),
        })
        .collect()
}

const fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}
