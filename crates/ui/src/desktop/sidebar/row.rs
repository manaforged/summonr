use std::rc::Rc;

use sigil::icons::Icon;
use sigil::prelude::{
    LayoutStyle, ListRowSlots, NodeBuilder, Signal, Tone, Widget, container, list_row,
};
use sigil::widgets::{MenuItem, icon};

use super::super::App;
use super::super::style::{ICON, M, S, SCROLLBAR, Text, mark, text};
use super::title;
use summonr_agent::endpoint::Friend;
use summonr_agent::hub::{Live, Open, Status};
use summonr_agent::prefs::Action;
use summonr_agent::session::SessionInfo;

const NEEDS_YOU: &str = "Needs you";
const PINNED: &str = "Pinned";
const UNPIN: &str = "Unpin";

/// The action rows a person pinned, in their order; right-click one to unpin it.
pub(super) fn actions(app: &App, pinned: Signal<Vec<Action>>) -> Vec<NodeBuilder> {
    pinned
        .get()
        .into_iter()
        .map(|action| action_row(app, action))
        .collect()
}

fn action_row(app: &App, action: Action) -> NodeBuilder {
    let (label, key, mark) = super::super::palette::row(action);
    let (run, menu) = (app.clone(), app.clone());
    let row = list_row(
        false,
        label,
        ListRowSlots {
            leading: Some(Box::new(icon(mark).px(ICON))),
            status: (!key.is_empty()).then(|| key.into()),
            activate: Some(Rc::new(move || super::super::palette::run(&run, action))),
            ..ListRowSlots::default()
        },
    );
    sigil::widgets::context_menu(row, move || {
        let unpin = menu.clone();
        std::iter::once(
            MenuItem::new(UNPIN).activate(move || super::super::palette::unpin(&unpin, action)),
        )
        .chain(add_items(&menu))
        .collect()
    })
}

/// The menu item that opens the list of actions to pin, or nothing when all are pinned.
pub(super) fn add_items(app: &App) -> Vec<MenuItem> {
    if super::super::palette::all_pinned(app) {
        return Vec::new();
    }
    vec![MenuItem::new(super::super::palette::PIN).activate(super::super::palette::show_pins)]
}

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
    shown: Signal<Vec<String>>,
) -> Vec<NodeBuilder> {
    let chats = app.sessions.all_chats();
    let rows: Vec<SessionInfo> = super::super::shelf::pinned()
        .iter()
        .filter_map(|id| chats.iter().find(|info| info.id == *id).cloned())
        .collect();
    if rows.is_empty() {
        return Vec::new();
    }
    let (live, shown) = (live.get(), shown.get());
    let heading = text(Text::Heading, PINNED)
        .layout(LayoutStyle::block().padding_top(M).padding_left(S))
        .into();
    std::iter::once(heading)
        .chain(rows.into_iter().map(|mut info| {
            super::live_title(&mut info, &live);
            let state = super::shown_state(&live, &info.id);
            agent_row(app, &info, shown.contains(&info.id), state)
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
    let friend = info.agent.as_deref().and_then(Friend::of);
    let word = friend.and_then(|_| status_word(status));
    let meta = if needs { None } else { info.modified.map(ago) };
    let meta = word.map(|(word, _)| word.to_owned()).or(meta);
    let row = list_row(
        selected,
        title(info),
        ListRowSlots {
            leading: Some(Box::new(leading(friend, status))),
            status: meta.map(Into::into),
            status_tone: word.map(|(_, tone)| tone),
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

fn leading(friend: Option<Friend>, status: Status) -> NodeBuilder {
    friend.map_or_else(|| mark(status), super::super::friends::glyph)
}

/// The short status word and tone an agent or topic row shows on the right.
pub(in crate::desktop) const fn status_word(status: Status) -> Option<(&'static str, Tone)> {
    match status {
        Status::Working => Some(("working", Tone::Neutral)),
        Status::Review => Some(("new", Tone::Info)),
        Status::Failed => Some(("failed", Tone::Error)),
        Status::NeedsYou | Status::Idle => None,
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
