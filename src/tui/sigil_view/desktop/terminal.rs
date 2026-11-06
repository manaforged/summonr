use std::path::Path;

use sigil::prelude::{LayoutStyle, NodeBuilder, Signal, Widget, container};

use super::App;
use super::style::{S, Text, text};
use crate::tui::sigil_view::model::Entry;

/// A shell in the active chat's folder, started the first time the tab opens there.
pub(super) fn view(app: &App, width: f32) -> NodeBuilder {
    let folder = app.workspace();
    let column = container().layout(
        LayoutStyle::flex_column()
            .width(width)
            .fill_height()
            .padding(S)
            .min_size(0.0, 0.0),
    );
    match lines(app, &folder) {
        Ok(lines) => {
            let typed = app.clone();
            column.child(
                sigil::widgets::terminal(lines, move |input| write(&typed, &folder, &input)).label("Shell"),
            )
        }
        Err(error) => column.child(text(Text::Hint, format!("Could not start a shell: {error}"))),
    }
}

fn lines(app: &App, folder: &Path) -> Result<Signal<Vec<String>>, String> {
    let mut shells = app.shells.borrow_mut();
    if let Some(session) = shells.get(folder) {
        return Ok(session.lines);
    }
    let owner = app.owner.ok_or_else(|| "the window has no owner".to_owned())?;
    let session = owner
        .with(|| sigil::app::Session::spawn_in(&shell(), folder))
        .map_err(|error| error.to_string())?;
    let lines = session.lines;
    shells.insert(folder.to_path_buf(), session);
    Ok(lines)
}

fn write(app: &App, folder: &Path, input: &str) {
    let failed = app
        .shells
        .borrow()
        .get(folder)
        .and_then(|session| session.write(input.as_bytes()).err());
    if let Some(error) = failed {
        app.shells.borrow_mut().remove(folder);
        app.push_entry(Entry::System(format!("The terminal stopped: {error}")));
    }
}

fn shell() -> String {
    std::env::var("SHELL")
        .or_else(|_| std::env::var("COMSPEC"))
        .unwrap_or_else(|_| "sh".to_owned())
}
