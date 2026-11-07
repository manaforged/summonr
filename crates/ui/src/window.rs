use std::path::Path;

use sigil::Host;
use sigil::prelude::{NodeBuilder, Signal};
use sigil::terminal::Grid;
use summonr_agent::hub::{Cmd, Msg};
use summonr_core::error::Result;
use tokio::sync::mpsc::UnboundedSender;

use crate::app::App;
use crate::chat::entry::Slot;
use crate::view::keys::bindings::{Jump, Split};

pub(crate) const CAPTURES_IMAGE: bool = false;

pub(crate) fn save_image(_grid: &Grid, _host: &Host, _output: &Path) -> Result<()> {
    Ok(())
}

/// What a window adds to the shared view. A terminal adds nothing, so every call here is inert.
#[derive(Clone)]
pub(crate) struct Window;

impl Window {
    pub(crate) fn new() -> Self {
        Self
    }

    pub(crate) fn palette(&self) -> bool {
        false
    }

    pub(crate) fn sign_in(&self, _app: &crate::app::App, _route: &str) -> bool {
        false
    }

    pub(crate) fn show_usage(&self, _app: &crate::app::App) -> bool {
        false
    }

    pub(crate) fn toggle_sidebar(&self) {}

    pub(crate) fn jump(&self, _app: &App, _to: Jump) {}

    pub(crate) fn open_terminal(&self, _app: &App) {}

    pub(crate) fn mark_unread(&self, _app: &App) {}

    pub(crate) fn split(&self, _action: Split) {}

    pub(crate) fn dialog_open(&self) -> bool {
        false
    }

    pub(crate) fn focused_pane(&self) -> Option<App> {
        None
    }

    pub(crate) fn showing_terminal(&self) -> bool {
        false
    }

    pub(crate) fn close_terminal(&self) {}

    pub(crate) fn call_attention(&self, _app: &App, _msg: &Msg) {}

    pub(crate) fn choose_chrome(&self, _cmd: &UnboundedSender<Cmd>) {}

    pub(crate) fn row(
        &self,
        _app: &App,
        _place: (Slot, usize),
        _open: Signal<Option<bool>>,
        _spacing: (f32, f32),
    ) -> Vec<NodeBuilder> {
        Vec::new()
    }
}
