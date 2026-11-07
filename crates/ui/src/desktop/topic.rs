use std::rc::Rc;

use sigil::prelude::{
    AlignItems, LayoutStyle, ListRowSlots, NodeBuilder, Tone, Widget, container, derived, effect,
    list_row, signal, tabs,
};
use sigil::reactive::untrack;
use summonr_agent::endpoint::Friend;
use summonr_agent::hub::{Live, Open, Status};
use summonr_agent::session::SessionInfo;

use super::App;
use super::style::{ICON, Text, text};

const STACK: usize = 3;
const OVERLAP: f32 = 5.0;
const NEEDS_YOU: &str = "Needs you";
const NEW_THREAD: &str = "New thread";
const THREADS: &str = "Threads";

/// The threads that joined the thread `id` started, newest first.
pub(super) struct Topic {
    pub(super) id: String,
    pub(super) threads: Vec<SessionInfo>,
}

/// The topic a thread belongs to: the thread that started it.
pub(super) fn key(info: &SessionInfo) -> &str {
    info.topic.as_deref().unwrap_or(&info.id)
}

/// Groups `threads` into topics, newest topic first.
pub(super) fn group(threads: Vec<SessionInfo>) -> Vec<Topic> {
    let mut topics: Vec<Topic> = Vec::new();
    for info in threads {
        let id = key(&info).to_owned();
        match topics.iter_mut().find(|topic| topic.id == id) {
            Some(topic) => topic.threads.push(info),
            None => topics.push(Topic {
                id,
                threads: vec![info],
            }),
        }
    }
    for topic in &mut topics {
        topic.threads.sort_by(|a, b| b.id.cmp(&a.id));
    }
    topics.sort_by(|a, b| b.id.cmp(&a.id));
    topics
}

impl Topic {
    /// The thread that started the topic, or its oldest listed thread.
    fn root(&self) -> Option<&SessionInfo> {
        self.threads
            .iter()
            .find(|info| info.id == self.id)
            .or_else(|| self.threads.last())
    }

    /// Each outside agent once, in the order its first thread started.
    fn agents(&self) -> Vec<Friend> {
        let mut seen: Vec<Friend> = Vec::new();
        for info in self.threads.iter().rev() {
            let agent = info.agent.as_deref().and_then(Friend::of);
            if let Some(agent) = agent.filter(|agent| !seen.iter().any(|known| known.name == agent.name)) {
                seen.push(agent);
            }
        }
        seen
    }

    /// The strongest state among the topic's threads.
    pub(super) fn state(&self, live: &[Live]) -> Option<Status> {
        self.threads
            .iter()
            .filter_map(|info| super::sidebar::shown_state(live, &info.id))
            .max_by_key(|status| rank(*status))
    }
}

const fn rank(status: Status) -> u8 {
    match status {
        Status::NeedsYou => 4,
        Status::Working => 3,
        Status::Failed => 2,
        Status::Review => 1,
        Status::Idle => 0,
    }
}

/// The sidebar row of a topic: its agents' marks, its first thread's title, and its state.
pub(super) fn row(app: &App, topic: &Topic, selected: bool, live: &[Live]) -> Option<NodeBuilder> {
    let mut root = topic.root()?.clone();
    super::sidebar::live_title(&mut root, live);
    let state = topic.state(live);
    let status = state.unwrap_or(Status::Idle);
    let needs = status == Status::NeedsYou;
    let word = word(status);
    let meta = if needs {
        None
    } else {
        topic.threads[0].modified.map(super::sidebar::row::ago)
    };
    let (open, newest) = (app.clone(), topic.threads[0].id.clone());
    let menu = (app.clone(), root.clone());
    let row = list_row(
        selected,
        super::sidebar::title(&root),
        ListRowSlots {
            leading: Some(Box::new(stack(&topic.agents()))),
            status: word.map(|(word, _)| word.to_owned()).or(meta).map(Into::into),
            status_tone: word.map(|(_, tone)| tone),
            badge: needs.then(|| (NEEDS_YOU.into(), Tone::Warn)),
            activate: Some(Rc::new(move || {
                super::split::show(&open, Open::Resume(newest.clone()))
            })),
            ..ListRowSlots::default()
        },
    );
    Some(sigil::widgets::context_menu(row, move || {
        super::agent_menu::items(&menu.0, &menu.1, state)
    }))
}

const fn word(status: Status) -> Option<(&'static str, Tone)> {
    match status {
        Status::Working => Some(("working", Tone::Neutral)),
        Status::Review => Some(("new", Tone::Info)),
        Status::Failed => Some(("failed", Tone::Error)),
        Status::NeedsYou | Status::Idle => None,
    }
}

/// Up to three outside agents' marks, overlapping, then `+N`; an empty slot for Summonr alone.
fn stack(agents: &[Friend]) -> NodeBuilder {
    let mut row = container().layout(
        LayoutStyle::flex_row()
            .min_size(ICON, ICON)
            .shrink(0.0)
            .align_items(AlignItems::Center),
    );
    for (at, agent) in agents.iter().take(STACK).enumerate() {
        let glyph = super::friends::glyph(*agent);
        let shift = if at == 0 { 0.0 } else { -OVERLAP };
        row = row.child(
            container()
                .layout(LayoutStyle::block().margin_left(shift).shrink(0.0))
                .child(glyph),
        );
    }
    if agents.len() > STACK {
        row = row.child(text(Text::Meta, format!("+{}", agents.len() - STACK)).nowrap());
    }
    row
}

/// The pane's thread tabs, shown once its topic has two threads, and a New thread button.
pub(super) fn strip(app: &App) -> NodeBuilder {
    let (watch, listed) = (app.clone(), app.clone());
    let threads = derived(move || {
        let session = watch.model.with(|model| model.session.clone());
        let chats = listed.sessions.all_chats();
        let Some(topic) = chats.iter().find(|info| info.id == session).map(key) else {
            return (session, Vec::new());
        };
        let topic = topic.to_owned();
        let mut members: Vec<SessionInfo> = chats.into_iter().filter(|info| key(info) == topic).collect();
        members.sort_by(|a, b| a.id.cmp(&b.id));
        (session, members)
    });
    let picked = signal(0usize);
    effect(move || {
        let (session, members) = threads.get();
        let at = members.iter().position(|info| info.id == session).unwrap_or(0);
        if untrack(|| picked.get()) != at {
            picked.set(at);
        }
    });
    let open = app.clone();
    effect(move || {
        let at = picked.get();
        let (session, members) = untrack(|| threads.get());
        if let Some(info) = members.get(at).filter(|info| info.id != session) {
            super::split::show(&open, Open::Resume(info.id.clone()));
        }
    });
    let labels = derived(move || {
        let (_, members) = threads.get();
        if members.len() < 2 {
            return Vec::new();
        }
        members.iter().map(super::sidebar::title).collect::<Vec<_>>()
    });
    container()
        .layout(
            LayoutStyle::flex_row()
                .shrink(1.0)
                .min_size(0.0, 0.0)
                .align_items(AlignItems::Center),
        )
        .child(tabs().labels(labels).active(picked).label(THREADS))
        .child(new_thread(app))
}

fn new_thread(app: &App) -> NodeBuilder {
    let open = app.clone();
    let button = sigil::prelude::icon_button(sigil::icons::Icon::Plus, move || {
        let session = open.model.with(|model| model.session.clone());
        let chats = open.sessions.all_chats();
        let Some(info) = chats.iter().find(|info| info.id == session) else {
            return super::split::show(&open, Open::New);
        };
        let folder = info.cwd.clone().map_or_else(|| open.workspace(), Into::into);
        super::split::show(
            &open,
            Open::Thread {
                topic: key(info).to_owned(),
                folder,
            },
        );
    })
    .accessible(NEW_THREAD);
    super::tip(NEW_THREAD, None, button.into())
}
