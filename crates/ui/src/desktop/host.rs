use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::Instant;

use sigil::Host;
use sigil::prelude::{NodeBuilder, Signal, effect, signal};
use sigil::terminal::Grid;
use summonr_agent::durable::store::{Durability, write};
use summonr_agent::hub::{Cmd, Msg};
use summonr_agent::session::SessionInfo;
use summonr_core::error::{Error, Result};
use tokio::sync::mpsc::UnboundedSender;

use super::terminal::{NEW_TERMINAL, Terminals};
use crate::app::App;
use crate::chat::entry::Slot;
use crate::chat::surface::inset;
use crate::model::{Entries, Entry};
use crate::sessions::Picker;
use crate::view::keys::bindings::{Jump, Split};

pub(crate) const CAPTURES_IMAGE: bool = true;

/// Tells the shared view that cells are pixels in a window.
pub(crate) fn window_cells() {
    crate::canvas::WINDOW.store(true, Ordering::Relaxed);
}

pub(crate) fn save_image(grid: &Grid, host: &Host, output: &Path) -> Result<()> {
    let image = grid
        .image(host.theme(), sigil::geometry::PhysicalSize::new(10, 20))
        .map_err(|error| Error::Tool(error.to_string()))?;
    let mut png = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(|error| Error::Tool(error.to_string()))?;
    write(
        &output.with_extension("png"),
        png.get_ref(),
        Durability::Renamed,
    )?;
    Ok(())
}

/// What a window adds to the shared view.
#[derive(Clone)]
pub(crate) struct Window {
    pub(crate) focused: Option<Signal<bool>>,
    pub(crate) terminals: Terminals,
    pub(crate) showing: Signal<Option<u64>>,
    pub(crate) sidebar: Signal<bool>,
    pub(crate) local: Signal<Vec<summonr_agent::endpoint::Local>>,
}

impl Window {
    pub(crate) fn new() -> Self {
        Self {
            focused: sigil::app::WindowId::current().and_then(sigil::app::WindowId::focused),
            terminals: Terminals::new(),
            showing: signal(None),
            sidebar: signal(true),
            local: signal(Vec::new()),
        }
    }

    pub(crate) fn palette(&self) -> bool {
        super::palette::show()
    }

    pub(crate) fn toggle_sidebar(&self) {
        self.sidebar.set(!self.sidebar.peek());
    }

    pub(crate) fn jump(&self, app: &App, to: Jump) {
        super::jump(app, to);
    }

    pub(crate) fn open_terminal(&self, app: &App) {
        app.open_terminal(NEW_TERMINAL, None);
    }

    pub(crate) fn split(&self, action: Split) {
        super::split::act(action);
    }

    pub(crate) fn dialog_open(&self) -> bool {
        super::dialog_open()
    }

    pub(crate) fn focused_pane(&self) -> Option<App> {
        super::split::focused()
    }

    pub(crate) fn showing_terminal(&self) -> bool {
        self.showing.peek().is_some()
    }

    pub(crate) fn close_terminal(&self) {
        self.showing.set(None);
    }

    pub(crate) fn call_attention(&self, app: &App, msg: &Msg) {
        app.call_attention(msg);
    }

    /// Asks for the Chrome-family browser the browser tools drive, and saves the choice.
    pub(crate) fn choose_chrome(&self, cmd: &UnboundedSender<Cmd>) {
        let mut dialog = sigil::app::dialog::FileDialog::new();
        if cfg!(target_os = "macos") {
            dialog = dialog
                .directory("/Applications")
                .filter("Applications", &["app"]);
        }
        let picked = dialog.open_file();
        let cmd = cmd.clone();
        effect(move || {
            if let sigil::tasks::Status::Ok(Some(path)) = picked.get() {
                drop(cmd.send(Cmd::SetChrome(path)));
            }
        });
    }

    /// The window's own drawing of one transcript slot.
    pub(crate) fn row(
        &self,
        app: &App,
        place: (Slot, usize),
        open: Signal<Option<bool>>,
        spacing: (f32, f32),
    ) -> Vec<NodeBuilder> {
        let (at, index) = place;
        match at {
            Slot::Member => Vec::new(),
            Slot::Lead(range)
                if app.model.with(|model| {
                    model
                        .entries
                        .get(range.start)
                        .is_some_and(crate::chat::entry::subagents)
                }) =>
            {
                vec![inset(super::entry::agents(app, range), spacing)]
            }
            Slot::Lead(range) => {
                vec![inset(
                    super::entry::fold::card(app, range, open.peek(), open),
                    spacing,
                )]
            }
            Slot::Single => super::entry::single(app, index)
                .map(|node| vec![inset(node, spacing)])
                .unwrap_or_default(),
        }
    }
}

impl App {
    /// Switches to a provider found on this machine, or lists its models when it has no default.
    pub(crate) fn use_local(&self, found: &summonr_agent::endpoint::Local) {
        if let Err(error) = summonr_agent::endpoint::select(&found.route) {
            self.push_entry(Entry::System(format!(
                "Could not use {}: {error}",
                found.name
            )));
            return;
        }
        if found.model.is_empty() {
            self.begin_model_edit();
            self.replace_model_draft(found.endpoint.clone());
            return;
        }
        drop(self.cmd.send(Cmd::SelectModel(
            found.endpoint.clone(),
            found.model.clone(),
        )));
    }

    /// The open overlay, skipping `skip` when a surface of its own shows it.
    pub(crate) fn overlay_beyond(&self, skip: crate::app::Overlay) -> Option<crate::app::Overlay> {
        crate::app::overlays::ROWS
            .iter()
            .filter(|row| row.overlay != skip)
            .find(|row| (row.open)(self).get())
            .map(|row| row.overlay)
    }

    pub(crate) fn fit_window(&self) {
        let size = sigil::app::viewport_size();
        let app = self.clone();
        effect(move || {
            let logical = size.get();
            let cell = crate::canvas::cell();
            if logical.width > 0.0 && logical.height > 0.0 {
                app.resize(
                    (logical.width / cell.w).floor().max(1.0) as u16,
                    (logical.height / cell.h).floor().max(1.0) as u16,
                );
            }
        });
    }
}

impl Picker {
    /// Top-level chats in every project from the last catalog read, newest first.
    pub(crate) fn all_chats(self) -> Vec<SessionInfo> {
        self.choices
            .get()
            .into_iter()
            .filter(|(info, _)| info.depth == 0 && info.interactive)
            .map(|(info, _)| info)
            .collect()
    }
}

impl Entries {
    /// The note texts while the chat holds nothing but notes; `None` once anything else arrives.
    pub(crate) fn only_notes(&self) -> Option<Vec<String>> {
        let mut notes = Vec::new();
        for entry in &self.inner {
            if let Entry::System(text) = entry {
                notes.push(text.clone());
            } else if !matches!(entry, Entry::Injected(_)) {
                return None;
            }
        }
        Some(notes)
    }
}

/// The status words and the start of the timed step, for a window that draws its own row.
pub(crate) fn summary(app: &App) -> Option<(String, Option<Instant>)> {
    crate::chat::strip::bar::status(app).map(|status| {
        let words = status
            .parts
            .iter()
            .map(|part| part.text.as_str())
            .collect::<Vec<_>>()
            .join(" · ");
        (words, status.clock.map(|(since, _)| since))
    })
}
