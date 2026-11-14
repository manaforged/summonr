#![recursion_limit = "256"]

use std::path::PathBuf;

use summonr_agent::provider::Client;
use summonr_agent::runtime::Loop;
use summonr_core::error::Result;

mod app;
mod canvas;
mod cells;
mod chat;
mod demo;
#[cfg(feature = "gui")]
mod desktop;
#[cfg(feature = "gui")]
use desktop::host as window;
mod effort;
mod help;
mod host;
mod matching;
mod text_style;
#[cfg(not(feature = "gui"))]
mod window;

mod model;
mod pane;
mod panes;
mod profile;
#[cfg(test)]
mod scale_tests;
mod scene;
mod search;
mod secret;
mod sessions;
mod settings;
mod speed;
mod suggest;
mod theme;
mod ui;
mod usage;
mod view;
mod will;

pub(crate) use matching::matching_commands;
pub use profile::run as profile;
pub(crate) use summonr_agent::display::tool_arg;

#[cfg(feature = "gui")]
pub use desktop::launch::run as desktop;

/// Run the terminal interface with the existing session hub.
pub fn run(
    loop_: Loop<Client>,
    workspace: PathBuf,
    info: summonr_agent::hub::Info,
    ui: summonr_agent::hub::Ui,
    inbox: summonr_agent::hub::Inbox,
    factory: summonr_agent::hub::Factory,
) -> Result<i32> {
    let (ui_tx, ui_rx) = (ui.0, inbox.0);
    host::run::run(loop_, workspace, info, ui_tx, ui_rx, factory)
}

const PRODUCT: &str = "summonr";

#[derive(Clone, Debug)]
enum ToolState {
    Running,
    Ok,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outcome {
    Running,
    Done,
    Conflict,
    Kept,
    Stopped,
    Failed,
    Interrupted,
}

impl Outcome {
    fn of(state: &summonr_agent::child::ChildState) -> Self {
        use summonr_agent::child::ChildState;
        use summonr_agent::tools::Hold;
        match state {
            ChildState::Running => Self::Running,
            ChildState::Completed(result) => match result.kept.as_ref().map(|kept| kept.hold) {
                Some(Hold::Conflict) => Self::Conflict,
                Some(Hold::OffBase | Hold::Unreadable | Hold::Unremoved) => Self::Kept,
                None => Self::Done,
            },
            ChildState::Aborted(_) => Self::Stopped,
            ChildState::Failed(_) => Self::Failed,
            ChildState::Interrupted(_) => Self::Interrupted,
        }
    }

    fn tool_state(self) -> ToolState {
        match self {
            Self::Running => ToolState::Running,
            Self::Done | Self::Conflict | Self::Kept => ToolState::Ok,
            Self::Stopped | Self::Failed | Self::Interrupted => ToolState::Failed,
        }
    }
}
