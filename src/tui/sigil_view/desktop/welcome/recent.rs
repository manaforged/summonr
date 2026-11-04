use std::collections::BTreeSet;
use std::path::PathBuf;

use sigil::prelude::{
    LayoutStyle, NodeBuilder, Paint, Signal, Token, Widget, button, column, container, signal,
    text, text_field,
};
use sigil::tasks::on_message;
use tokio::sync::mpsc::unbounded_channel;

use super::{L, Open, S};
use crate::session::SessionInfo;

/// Render searchable workspaces from the existing session catalog.
pub(super) fn list(busy: Signal<bool>, error: Signal<Option<String>>, open: Open) -> NodeBuilder {
    let rows = signal(Vec::<SessionInfo>::new());
    let loading = signal(true);
    let query = signal(String::new());
    let (out, inbox) = unbounded_channel();
    on_message(
        inbox,
        move |result: crate::Result<Vec<SessionInfo>>| {
            loading.set(false);
            match result {
                Ok(found) => rows.set(found),
                Err(cause) => error.set(Some(format!("Could not load workspace history: {cause}"))),
            }
        },
        || {},
    );
    tokio::task::spawn_blocking(move || drop(out.send(crate::session::all())));
    column()
        .layout(LayoutStyle::flex_column().gap(L))
        .child(text("Recent workspaces").font_size(14.0).font_weight(600))
        .child(text_field(query).placeholder("Search workspaces"))
        .child(
            container()
                .layout(LayoutStyle::flex_column().gap(S))
                .children_reactive(move || {
                    let paths = paths(&rows.get(), &query.get());
                    if paths.is_empty() {
                        let message = if loading.get() {
                            "Loading workspaces…"
                        } else {
                            "No matching workspaces. Open a folder above."
                        };
                        return vec![text(message).text_color(Token::OnSurfaceMuted).into()];
                    }
                    paths
                        .into_iter()
                        .map(|path| workspace(path, busy, open.clone()))
                        .collect()
                }),
        )
}

fn paths(rows: &[SessionInfo], query: &str) -> Vec<PathBuf> {
    let query = query.trim().to_lowercase();
    let mut seen = BTreeSet::new();
    rows.iter()
        .rev()
        .filter(|row| row.depth == 0)
        .filter_map(|row| {
            let path = PathBuf::from(row.cwd.as_ref()?);
            (path.to_string_lossy().to_lowercase().contains(&query) && seen.insert(path.clone()))
                .then_some(path)
        })
        .collect()
}

fn workspace(path: PathBuf, busy: Signal<bool>, open: Open) -> NodeBuilder {
    let available = path.is_dir();
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let title = if available {
        name
    } else {
        format!("{name} — unavailable")
    };
    let display = crate::tilde(&path);
    column()
        .layout(LayoutStyle::flex_column().gap(S))
        .child(
            button(title)
                .ghost()
                .disabled(move || busy.get() || !available)
                .activate(move || open(path.clone())),
        )
        .child(
            text(display)
                .font_size(13.0)
                .text_color(Token::OnSurfaceMuted),
        )
        .into()
}
