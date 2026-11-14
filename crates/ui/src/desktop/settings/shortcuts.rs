use sigil::prelude::{
    AlignItems, ModifiersState, NodeBuilder, Paint, Radius, Widget, button, derived,
};

use super::super::App;
use super::super::style::{OUTLINE, S, TINT, Text, XL, XS, text};
use super::parts::{actions, row, section};
use crate::pane::Pane;
use crate::view::KeyCommand;
use crate::view::keys::bindings::{Jump, Split, WINDOW};
use summonr_agent::prefs::{Hotkey, Hotkeys};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Group {
    Agents,
    Window,
    Composer,
    Transcript,
}

impl Group {
    const ALL: [Self; 4] = [Self::Agents, Self::Window, Self::Composer, Self::Transcript];

    const fn name(self) -> &'static str {
        match self {
            Self::Agents => "Agents",
            Self::Window => "Window",
            Self::Composer => "Composer",
            Self::Transcript => "Transcript",
        }
    }
}

type Chord = Vec<String>;
type Binding = Vec<Chord>;

#[derive(Clone, PartialEq)]
struct Shortcut {
    group: Group,
    label: &'static str,
    keys: Vec<Binding>,
}

const CMD: ModifiersState = ModifiersState::META;
const CTRL: ModifiersState = ModifiersState::CONTROL;
const CMD_ALT: ModifiersState = ModifiersState::META.union(ModifiersState::ALT);
const CTRL_SHIFT: ModifiersState = ModifiersState::CONTROL.union(ModifiersState::SHIFT);

const CMD_SHIFT: ModifiersState = ModifiersState::META.union(ModifiersState::SHIFT);
const CMD_ALT_SHIFT: ModifiersState = CMD_ALT.union(ModifiersState::SHIFT);

const STEPS: [(Group, &str, ModifiersState, &str); 6] = [
    (Group::Agents, "Next agent", CTRL, "Tab"),
    (Group::Agents, "Previous agent", CTRL_SHIFT, "Tab"),
    (Group::Agents, "Go to agent 1–9", CMD, "1–9"),
    (Group::Window, "Focus pane", CMD_ALT, "←↑↓→"),
    (Group::Window, "Swap pane", CMD_ALT_SHIFT, "←↑↓→"),
    (Group::Window, "Zoom pane", CMD_SHIFT, "Return"),
];

pub(super) fn view(app: &App) -> Vec<NodeBuilder> {
    let model = app.model;
    let list = derived(move || shortcuts(&model.get().prefs.hotkeys));
    let edit = app.clone();
    vec![
        sigil::prelude::column()
            .fill_width()
            .align_items(AlignItems::Stretch)
            .gap(XL)
            .children_reactive(move || {
                let list = list.get();
                Group::ALL
                    .into_iter()
                    .map(|group| group_section(group, &list))
                    .collect::<Vec<_>>()
            }),
        section(
            None,
            vec![row(
                "Custom keys",
                Some("Change the composer and transcript keys"),
                button("Edit…").activate(move || edit.open_hotkeys()).into(),
            )],
        ),
    ]
}

fn group_section(group: Group, list: &[Shortcut]) -> NodeBuilder {
    let rows = list
        .iter()
        .filter(|shortcut| shortcut.group == group)
        .map(|shortcut| row(shortcut.label, None, keys(&shortcut.keys)))
        .collect();
    section(Some(group.name()), rows)
}

fn shortcuts(hotkeys: &Hotkeys) -> Vec<Shortcut> {
    let mut list = Vec::new();
    for &(key, mods, command) in &WINDOW {
        if let Some((group, label)) = window_label(command).filter(|_| key.is_ascii()) {
            add(
                &mut list,
                group,
                label,
                vec![chord(mods, &key.to_uppercase())],
            );
        }
    }
    for (group, label, mods, key) in STEPS {
        add(&mut list, group, label, vec![chord(mods, key)]);
    }
    for action in Hotkey::ALL {
        let (group, label) = hotkey_label(action);
        add(&mut list, group, label, binding(hotkeys, action));
    }
    list
}

fn add(list: &mut Vec<Shortcut>, group: Group, label: &'static str, keys: Binding) {
    match list.iter_mut().find(|shortcut| shortcut.label == label) {
        Some(found) if !found.keys.contains(&keys) => found.keys.push(keys),
        Some(_) => {}
        None => list.push(Shortcut {
            group,
            label,
            keys: vec![keys],
        }),
    }
}

fn binding(hotkeys: &Hotkeys, action: Hotkey) -> Binding {
    let key = hotkeys.key(action);
    let letter = key.to_uppercase().to_string();
    if hotkeys.chord(key) == Some(action) {
        vec![chord(CTRL, "X"), vec![letter]]
    } else {
        vec![chord(CTRL, &letter)]
    }
}

fn chord(mods: ModifiersState, key: &str) -> Chord {
    let mut caps = Vec::new();
    let names = modifier_names();
    for (flag, name) in [
        (ModifiersState::CONTROL, names[0]),
        (ModifiersState::ALT, names[1]),
        (ModifiersState::SHIFT, names[2]),
        (ModifiersState::META, names[3]),
    ] {
        if mods.contains(flag) {
            caps.push(name.to_owned());
        }
    }
    caps.push(key.to_owned());
    caps
}

const fn modifier_names() -> [&'static str; 4] {
    if cfg!(target_os = "macos") {
        ["⌃", "⌥", "⇧", "⌘"]
    } else {
        ["Ctrl", "Alt", "Shift", "Super"]
    }
}

const WINDOW_LABELS: [(KeyCommand, Group, &str); 15] = [
    (KeyCommand::NewSession, Group::Agents, "New agent"),
    (KeyCommand::Palette, Group::Agents, "Search agents"),
    (
        KeyCommand::Jump(Jump::Needing),
        Group::Agents,
        "Next that needs you",
    ),
    (KeyCommand::Interrupt, Group::Agents, "Stop agent"),
    (KeyCommand::MarkUnread, Group::Agents, "Mark unread"),
    (KeyCommand::OpenPr, Group::Agents, "Open PR"),
    (KeyCommand::Terminal, Group::Window, "New terminal"),
    (KeyCommand::Split(Split::Open), Group::Window, "Split pane"),
    (
        KeyCommand::Split(Split::Below),
        Group::Window,
        "Split pane down",
    ),
    (KeyCommand::Split(Split::Close), Group::Window, "Close pane"),
    (KeyCommand::Sidebar, Group::Window, "Show sidebar"),
    (KeyCommand::Pane(Pane::Diff), Group::Window, "Show changes"),
    (
        KeyCommand::Pane(Pane::Terminal),
        Group::Window,
        "Show terminal",
    ),
    (
        KeyCommand::Hotkey(Hotkey::Options),
        Group::Window,
        "Open settings",
    ),
    (KeyCommand::Trigger('@'), Group::Composer, "Mention a file"),
];

fn window_label(command: KeyCommand) -> Option<(Group, &'static str)> {
    WINDOW_LABELS
        .iter()
        .find(|row| row.0 == command)
        .map(|&(_, group, label)| (group, label))
}

const fn hotkey_label(action: Hotkey) -> (Group, &'static str) {
    match action {
        Hotkey::Options => (Group::Window, "Open settings"),
        Hotkey::Files => (Group::Window, "Show files"),
        Hotkey::Diff => (Group::Window, "Show changes"),
        Hotkey::Pane => (Group::Window, "Show side panel"),
        Hotkey::Newline => (Group::Composer, "New line"),
        Hotkey::Editor => (Group::Composer, "Open in editor"),
        Hotkey::History => (Group::Composer, "Search past prompts"),
        Hotkey::Copy => (Group::Transcript, "Copy last reply"),
        Hotkey::Search => (Group::Transcript, "Search this agent"),
        Hotkey::Select => (Group::Transcript, "Select text"),
        Hotkey::Fold => (Group::Transcript, "Fold details"),
        Hotkey::Ledger => (Group::Transcript, "Show tasks"),
        Hotkey::Agents => (Group::Transcript, "Show helper agents"),
    }
}

fn keys(bindings: &[Binding]) -> NodeBuilder {
    let mut line = actions();
    for (at, binding) in bindings.iter().enumerate() {
        if at > 0 {
            line = line.child(text(Text::Meta, "or"));
        }
        for (step, chord) in binding.iter().enumerate() {
            if step > 0 {
                line = line.child(text(Text::Meta, "then"));
            }
            line = line.child(caps(chord));
        }
    }
    line
}

fn caps(chord: &[String]) -> NodeBuilder {
    chord.iter().fold(
        sigil::prelude::row()
            .align_items(sigil::prelude::AlignItems::Stretch)
            .gap(XS),
        |line, cap| line.child(keycap(cap)),
    )
}

fn keycap(cap: &str) -> NodeBuilder {
    sigil::prelude::row()
        .gap(sigil::prelude::Space::None)
        .align_items(sigil::prelude::AlignItems::Stretch)
        .padding_x(S)
        .padding_y(XS)
        .background(TINT)
        .radius(Radius::Md)
        .border_width(1)
        .border_color(OUTLINE)
        .child(text(Text::Label, cap.to_owned()))
}
