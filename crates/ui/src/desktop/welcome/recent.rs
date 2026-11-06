use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use sigil::prelude::{
    LayoutStyle, ListRowSlots, NodeBuilder, Paint, Signal, Widget, column, container, list_row,
    signal,
};
use sigil::tasks::on_message;
use tokio::sync::mpsc::unbounded_channel;

use super::super::style::{DIVIDER, Text, XS};
use super::{GAP, Open, S};
use sigil::icons::Icon;
use sigil::widgets::icon;
use summonr_agent::session::SessionInfo;

const ROWS_MAX: usize = 5;

/// The most recent project folders that still exist, newest first.
pub(super) fn list(busy: Signal<bool>, error: Signal<Option<String>>, open: Open) -> NodeBuilder {
    let rows = signal(Vec::<SessionInfo>::new());
    let (out, inbox) = unbounded_channel();
    on_message(
        inbox,
        move |result: summonr_agent::Result<Vec<SessionInfo>>| match result {
            Ok(found) => rows.set(found),
            Err(cause) => error.set(Some(format!("Could not load recent folders: {cause}"))),
        },
        || {},
    );
    tokio::task::spawn_blocking(move || drop(out.send(summonr_agent::session::all())));
    container().children_reactive(move || -> Vec<NodeBuilder> {
        let paths = paths(&rows.get(), &temp_roots());
        if paths.is_empty() {
            return Vec::new();
        }
        let heading = container()
            .layout(LayoutStyle::block().padding_x(S))
            .child(super::super::style::text(Text::Heading, "Recent"));
        let list = paths
            .into_iter()
            .fold(column().layout(LayoutStyle::flex_column()), |list, path| {
                list.child(folder(path, busy, open.clone()))
            });
        vec![
            column()
                .layout(LayoutStyle::flex_column().gap(XS).padding_top(GAP))
                .border_top(1)
                .border_color(DIVIDER)
                .child(
                    container()
                        .layout(LayoutStyle::block().padding_top(GAP))
                        .child(heading),
                )
                .child(list)
                .into(),
        ]
    })
}

/// The folder of the newest chat that still exists.
pub(crate) fn last() -> Option<PathBuf> {
    let rows = summonr_agent::session::all().ok()?;
    paths(&rows, &temp_roots()).into_iter().next()
}

fn paths(rows: &[SessionInfo], temp: &[PathBuf]) -> Vec<PathBuf> {
    let mut seen = BTreeSet::new();
    rows.iter()
        .rev()
        .filter(|row| row.depth == 0)
        .filter_map(|row| row.cwd.as_ref().map(PathBuf::from))
        .filter(|path| seen.insert(path.clone()) && path.is_dir() && !scratch(path, temp))
        .take(ROWS_MAX)
        .collect()
}

fn temp_roots() -> Vec<PathBuf> {
    [std::env::temp_dir(), PathBuf::from("/tmp")]
        .iter()
        .filter_map(|root| root.canonicalize().ok())
        .collect()
}

fn scratch(path: &Path, temp: &[PathBuf]) -> bool {
    path.canonicalize()
        .is_ok_and(|path| temp.iter().any(|root| path.starts_with(root)))
}

fn folder(path: PathBuf, busy: Signal<bool>, open: Open) -> NodeBuilder {
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let place = path.parent().map(summonr_agent::tilde).unwrap_or_default();
    list_row(
        false,
        name,
        ListRowSlots {
            leading: Some(Box::new(icon(Icon::Folder).px(14.0))),
            detail: Some(place.into()),
            activate: Some(Rc::new(move || {
                if !busy.peek() {
                    open(path.clone());
                }
            })),
            ..ListRowSlots::default()
        },
    )
}
