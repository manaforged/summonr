use sigil::Host;
use sigil::app::FrameHooks;
use sigil::events::{
    EventRouter, KeyEvent, KeyEventInit, KeyPhase, LogicalKey, ModifiersState, PhysicalKey,
    ShellEvent,
};
use sigil::kernel::Role;
use sigil::prelude::Key;
use sigil::reactive::Runtime;
use sigil::scene::SceneCommand;
use tokio::sync::mpsc::unbounded_channel;

use super::super::{App, root};
use super::VIEW;
use crate::provider::Effort;
use crate::tui::{Cmd, Msg, Snapshot};

pub(super) struct Seen<T> {
    pub(super) read: T,
    pub(super) sent: Vec<Cmd>,
    pub(super) controls: Vec<(Role, String, bool)>,
    pub(super) texts: Vec<String>,
}

impl<T> Seen<T> {
    pub(super) fn text(&self) -> String {
        self.texts.join(" | ")
    }

    pub(super) fn named(&self, role: Role, label: &str) -> Vec<bool> {
        self.controls
            .iter()
            .filter(|(at, name, _)| *at == role && name == label)
            .map(|(_, _, disabled)| *disabled)
            .collect()
    }

    pub(super) fn shows(&self, wanted: impl Fn(&str) -> bool) -> bool {
        self.texts.iter().any(|text| wanted(text))
    }
}

fn press(key: LogicalKey, modifiers: ModifiersState, text: Option<&str>) -> ShellEvent {
    ShellEvent::Key(KeyEvent::new(KeyEventInit {
        key,
        physical: PhysicalKey::Unidentified,
        modifiers,
        repeat: false,
        phase: KeyPhase::Down,
        text: text.map(Into::into),
    }))
}

pub(super) fn named(key: Key, modifiers: ModifiersState) -> ShellEvent {
    press(LogicalKey::named(key), modifiers, None)
}

pub(super) fn command(letter: &str) -> ShellEvent {
    press(LogicalKey::character(letter), ModifiersState::META, None)
}

pub(super) fn drive<T>(
    setup: impl FnOnce(&App),
    keys: Vec<ShellEvent>,
    read: impl FnOnce(&App) -> T,
) -> Seen<T> {
    Runtime::scope("summonr-window", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let (commands, mut receiver) = unbounded_channel();
        let app = App::new(
            crate::tui::sigil_view::profile::info(),
            workspace.path().to_owned(),
            commands,
        );
        setup(&app);
        app.refresh();
        let mut host = Host::new();
        let tree = app.clone();
        host.mount(move || root(tree.clone()));
        let mut router = EventRouter::new();
        for _ in 0..3 {
            host.frame(&mut router, VIEW, 1.0, FrameHooks::default());
        }
        for key in keys {
            host.route(&mut router, key);
            host.frame(&mut router, VIEW, 1.0, FrameHooks::default());
        }
        let read = read(&app);
        host.frame(&mut router, VIEW, 1.0, FrameHooks::default());
        let mut sent = Vec::new();
        while let Ok(cmd) = receiver.try_recv() {
            sent.push(cmd);
        }
        let controls = host
            .accessibility_tree(&router)
            .nodes
            .iter()
            .filter_map(|(_, node)| {
                node.label()
                    .map(|label| (node.role(), label.to_string(), node.is_disabled()))
            })
            .collect();
        let texts = host
            .scene(VIEW)
            .expect("scene")
            .commands
            .into_iter()
            .filter_map(|command| {
                let SceneCommand::GlyphRun(draw) = command else {
                    return None;
                };
                Some(draw.run.text.clone())
            })
            .collect();
        Seen {
            read,
            sent,
            controls,
            texts,
        }
    })
}

pub(super) fn settings(snapshot: Snapshot) -> impl FnOnce(&App) {
    move |app| app.update(Msg::Settings(snapshot))
}

pub(super) fn graded() -> Snapshot {
    Snapshot {
        model: "stub-model".to_owned(),
        effort: Some(Effort::Low),
        levels: vec![Effort::Low, Effort::High],
        ..Snapshot::default()
    }
}

pub(super) fn unseated() -> Snapshot {
    Snapshot {
        unseated: true,
        ..Snapshot::default()
    }
}

pub(super) fn changed(base: &str, files: &[(&str, usize, usize)]) -> crate::diff::Diff {
    crate::diff::Diff {
        base: base.to_owned(),
        session: None,
        files: files
            .iter()
            .map(|(path, added, removed)| crate::diff::Stat {
                path: (*path).to_owned(),
                added: *added as _,
                removed: *removed as _,
                new: false,
            })
            .collect(),
    }
}
