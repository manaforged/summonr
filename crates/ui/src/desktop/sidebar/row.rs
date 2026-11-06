use std::rc::Rc;

use sigil::prelude::{ListRowSlots, NodeBuilder, Tone, list_row};

use super::super::App;
use super::super::style::mark;
use super::title;
use summonr_agent::hub::{Open, Status};
use summonr_agent::session::SessionInfo;

const NEEDS_YOU: &str = "Needs you";

pub(super) fn agent_row(
    app: &App,
    info: &SessionInfo,
    selected: bool,
    state: Option<Status>,
) -> NodeBuilder {
    let open = app.clone();
    let id = info.id.clone();
    let menu = (app.clone(), info.clone());
    let status = state.unwrap_or(Status::Idle);
    let needs = status == Status::NeedsYou;
    let meta = if needs { None } else { info.modified.map(ago) };
    let row = list_row(
        selected,
        title(info),
        ListRowSlots {
            leading: Some(Box::new(mark(status))),
            status: meta.map(Into::into),
            badge: needs.then(|| (NEEDS_YOU.into(), Tone::Warn)),
            activate: Some(Rc::new(move || {
                super::super::split::show(&open, Open::Resume(id.clone()))
            })),
            ..ListRowSlots::default()
        },
    );
    sigil::widgets::context_menu(row, move || {
        super::super::agent_menu::items(&menu.0, &menu.1, state)
    })
}

/// How long ago `when` was, in the short form agent lists use: now, 5m, 3h, 2d, 6w.
fn ago(when: std::time::SystemTime) -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(when)
        .map_or(0, |elapsed| elapsed.as_secs());
    match secs {
        0..60 => "now".to_string(),
        60..3_600 => format!("{}m", secs / 60),
        3_600..86_400 => format!("{}h", secs / 3_600),
        86_400..604_800 => format!("{}d", secs / 86_400),
        604_800.. => format!("{}w", secs / 604_800),
    }
}
