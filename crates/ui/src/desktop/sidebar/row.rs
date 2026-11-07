use std::rc::Rc;

use sigil::icons::Icon;
use sigil::prelude::{
    LayoutStyle, ListRowSlots, NodeBuilder, Signal, Tone, Widget, container, list_row,
};
use sigil::widgets::icon;

use super::super::App;
use super::super::style::{ICON, M, S, SCROLLBAR, Text, mark, text};
use super::title;
use summonr_agent::endpoint::Friend;
use summonr_agent::hub::{Live, Open, Status};
use summonr_agent::session::SessionInfo;

const NEEDS_YOU: &str = "Needs you";
const PINNED: &str = "Pinned";

/// The agents the repository lists show: not archived and not pinned.
pub(super) fn listed(app: &App) -> Vec<SessionInfo> {
    let mut chats = app.sessions.all_chats();
    chats.retain(|info| !super::super::shelf::set_aside(&info.id));
    chats
}

/// The Pinned heading and its agents, or nothing with no pins.
pub(super) fn pinned(
    app: &App,
    live: Signal<Vec<Live>>,
    session: Signal<String>,
) -> Vec<NodeBuilder> {
    let chats = app.sessions.all_chats();
    let rows: Vec<SessionInfo> = super::super::shelf::pinned()
        .iter()
        .filter_map(|id| chats.iter().find(|info| info.id == *id).cloned())
        .collect();
    if rows.is_empty() {
        return Vec::new();
    }
    let (live, session) = (live.get(), session.get());
    let heading = text(Text::Heading, PINNED)
        .layout(LayoutStyle::block().padding_top(M).padding_left(S))
        .into();
    std::iter::once(heading)
        .chain(rows.into_iter().map(|mut info| {
            super::live_title(&mut info, &live);
            let state = super::shown_state(&live, &info.id);
            agent_row(app, &info, info.id == session, state)
        }))
        .collect()
}

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
            leading: Some(Box::new(leading(info, status))),
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

fn leading(info: &SessionInfo, status: Status) -> NodeBuilder {
    let friend = info.agent.as_deref().and_then(Friend::of);
    match friend.filter(|_| status == Status::Idle) {
        Some(friend) => sigil::widgets::tooltip(friend.name)
            .child(mark(status).child(text(Text::Meta, friend.mark)))
            .into(),
        None => mark(status),
    }
}

/// How long ago `when` was, in the short form agent lists use: now, 5m, 3h, 2d, 6w.
pub(in crate::desktop) fn ago(when: std::time::SystemTime) -> String {
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

pub(super) fn footer(app: &App) -> NodeBuilder {
    let settings = app.settings_open;
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .padding_right(SCROLLBAR),
        )
        .child(list_row(
            false,
            "Settings",
            ListRowSlots {
                leading: Some(Box::new(icon(Icon::Settings).px(ICON))),
                status: Some("⌘,".into()),
                activate: Some(Rc::new(move || settings.set(true))),
                ..ListRowSlots::default()
            },
        ))
}
