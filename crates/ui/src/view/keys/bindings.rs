use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyCommand {
    Hotkey(Hotkey),
    Trigger(char),
    NewSession,
    MarkUnread,
    OpenPr,
    Palette,
    Pane(Pane),
    Always,
    Retry,
    Interrupt,
    Quit,
    ToggleRecent,
    CopyPatch,
    ViewExternal,
    Collapse,
    Expand,
    TabOrPane(bool),
    NudgeSplit(i16),
    HunkAccept,
    HunkReject,
    Shortcuts,
    Answer(Reply, GrantAnswer),
    Escape,
    Enter,
    Edit,
    Save,
    Note,
    Edge(bool),
    CtrlEdge(bool),
    Page(isize),
    Arrow(isize),
    Adjust(isize),
    Toggle,
    Recall(bool),
    Pick(usize),
    Sidebar,
    Jump(Jump),
    Split(Split),
    Terminal,
}

impl KeyCommand {
    /// Whether the key acts on the window rather than the focused pane.
    pub(super) const fn windowed(self) -> bool {
        matches!(
            self,
            Self::Palette
                | Self::Sidebar
                | Self::Jump(_)
                | Self::Split(_)
                | Self::Terminal
                | Self::Hotkey(Hotkey::Options)
        )
    }
}

/// What a split key does to the window's panes.
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    not(feature = "gui"),
    allow(dead_code, reason = "window pane shortcuts run only under gui")
)]
pub(crate) enum Split {
    /// Adds a pane right of the focused one, with a new agent.
    Open,
    /// Adds a pane below the focused one, with a new agent.
    Below,
    /// Closes the focused pane; its agent keeps running.
    Close,
    /// Moves focus to the nearest pane in a direction.
    Focus(Dir),
    /// Swaps the focused pane with the nearest pane in a direction.
    Swap(Dir),
    /// Shows only the focused pane, or every pane again.
    Zoom,
}

/// A direction on screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(
    not(feature = "gui"),
    allow(dead_code, reason = "window pane directions run only under gui")
)]
pub(crate) enum Dir {
    Left,
    Right,
    Up,
    Down,
}

/// Which agent a window shortcut opens.
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    not(feature = "gui"),
    allow(dead_code, reason = "window sidebar jumps run only under gui")
)]
pub(crate) enum Jump {
    /// The nth agent in sidebar order, from 1.
    Nth(usize),
    /// The next (true) or previous agent in sidebar order.
    Step(bool),
    /// The next agent that needs the user.
    Needing,
}

#[derive(Clone, Copy)]
pub(super) enum When {
    Always,
    Persist,
    Free,
    Unrecorded,
    Idle,
    DiffPane,
    ViewPane,
    PaneActive,
    HunkOpen,
    Window,
}

impl When {
    pub(super) fn holds(self, app: &App) -> bool {
        match self {
            Self::Always => true,
            Self::Persist => {
                app.answering() && app.approval.peek().is_some_and(|asked| asked.persist)
            }
            Self::Free => !app.options_active() && !app.view_edit.peek(),
            Self::Unrecorded => !app.hotkeys.recording.peek(),
            Self::Idle => {
                !app.hotkeys.recording.peek()
                    && !app.options_active()
                    && app.draft.peek().is_empty()
                    && app.model.get().busy_since.is_none()
            }
            Self::DiffPane => app.pane_active() == Some(Pane::Diff),
            Self::ViewPane => app.pane_active() == Some(Pane::View),
            Self::PaneActive => app.pane_active().is_some(),
            Self::HunkOpen => {
                app.pane_active() == Some(Pane::Diff) && app.diff_view.peek().is_some()
            }
            Self::Window => canvas::is_window(),
        }
    }
}

pub(super) fn with_bindings<R>(run: impl FnOnce(&[(Shortcut, When, KeyCommand)]) -> R) -> R {
    thread_local! {
        static TABLE: Vec<(Shortcut, When, KeyCommand)> = bindings();
    }
    TABLE.with(|table| run(table))
}

fn bindings() -> Vec<(Shortcut, When, KeyCommand)> {
    use KeyCommand as Do;
    let text = |key: &'static str| -> Shortcut { LogicalKey::character(key).into() };
    let ctrl = |key: &'static str| Shortcut::ctrl(LogicalKey::character(key));
    let shift =
        |key: &'static str| Shortcut::new(LogicalKey::character(key), ModifiersState::SHIFT);
    let ctrl_shift = ModifiersState::CONTROL | ModifiersState::SHIFT;
    let mut rows = vec![
        (shift("y"), When::Persist, Do::Always),
        (Key::F5.into(), When::Free, Do::Retry),
        (ctrl("c"), When::Unrecorded, Do::Interrupt),
        (ctrl("d"), When::Idle, Do::Quit),
        (text("t"), When::DiffPane, Do::ToggleRecent),
        (text("c"), When::DiffPane, Do::CopyPatch),
        (text("o"), When::ViewPane, Do::ViewExternal),
        (Key::ArrowLeft.into(), When::PaneActive, Do::Collapse),
        (Key::ArrowRight.into(), When::PaneActive, Do::Expand),
        (
            Shortcut::ctrl(Key::ArrowLeft),
            When::Always,
            Do::TabOrPane(false),
        ),
        (
            Shortcut::ctrl(Key::ArrowRight),
            When::Always,
            Do::TabOrPane(true),
        ),
        (
            Shortcut::new(Key::ArrowLeft, ctrl_shift),
            When::Always,
            Do::NudgeSplit(-5),
        ),
        (
            Shortcut::new(Key::ArrowRight, ctrl_shift),
            When::Always,
            Do::NudgeSplit(5),
        ),
        (text("a"), When::HunkOpen, Do::HunkAccept),
        (text("r"), When::HunkOpen, Do::HunkReject),
        (text("?"), When::Always, Do::Shortcuts),
        (shift("?"), When::Always, Do::Shortcuts),
        (
            text("y"),
            When::Always,
            Do::Answer(Reply::Send, GrantAnswer::Allow),
        ),
        (
            text("n"),
            When::Always,
            Do::Answer(Reply::Decline, GrantAnswer::Deny),
        ),
        (text("1"), When::Window, Do::Pick(1)),
        (text("2"), When::Window, Do::Pick(2)),
        (text("3"), When::Window, Do::Pick(3)),
        (text("4"), When::Window, Do::Pick(4)),
        (Key::Escape.into(), When::Always, Do::Escape),
        (Key::Enter.into(), When::Always, Do::Enter),
        (text("e"), When::Always, Do::Edit),
        (ctrl("s"), When::Always, Do::Save),
        (text("n"), When::Always, Do::Note),
        (Key::Home.into(), When::Always, Do::Edge(false)),
        (Key::End.into(), When::Always, Do::Edge(true)),
        (Shortcut::ctrl(Key::Home), When::Always, Do::CtrlEdge(false)),
        (Shortcut::ctrl(Key::End), When::Always, Do::CtrlEdge(true)),
        (Key::PageUp.into(), When::Always, Do::Page(-1)),
        (Key::PageDown.into(), When::Always, Do::Page(1)),
        (Key::ArrowUp.into(), When::Always, Do::Arrow(-1)),
        (Key::ArrowDown.into(), When::Always, Do::Arrow(1)),
        (Key::ArrowLeft.into(), When::Always, Do::Adjust(-1)),
        (Key::ArrowRight.into(), When::Always, Do::Adjust(1)),
        (text(" "), When::Always, Do::Toggle),
        (Key::ArrowUp.into(), When::Always, Do::Recall(true)),
        (Key::ArrowDown.into(), When::Always, Do::Recall(false)),
    ];
    rows.extend(WINDOW.iter().map(|&(key, mods, command)| {
        (
            Shortcut::new(LogicalKey::character(key), mods),
            When::Window,
            command,
        )
    }));
    let ctrl_tab = |mods| Shortcut::new(Key::Tab, mods);
    rows.push((
        ctrl_tab(ModifiersState::CONTROL),
        When::Window,
        Do::Jump(Jump::Step(true)),
    ));
    rows.push((
        ctrl_tab(ctrl_shift),
        When::Window,
        Do::Jump(Jump::Step(false)),
    ));
    for (key, dir) in ARROWS {
        rows.push((
            Shortcut::new(key, CMD_ALT),
            When::Window,
            Do::Split(Split::Focus(dir)),
        ));
        rows.push((
            Shortcut::new(key, CMD_ALT_SHIFT),
            When::Window,
            Do::Split(Split::Swap(dir)),
        ));
    }
    rows.push((
        Shortcut::new(Key::Enter, CMD_SHIFT),
        When::Window,
        Do::Split(Split::Zoom),
    ));
    rows.extend((1..=9).map(|n| {
        (
            Shortcut::new(LogicalKey::character(DIGITS[n - 1]), ModifiersState::META),
            When::Window,
            Do::Jump(Jump::Nth(n)),
        )
    }));
    rows
}

const DIGITS: [&str; 9] = ["1", "2", "3", "4", "5", "6", "7", "8", "9"];
const CMD: ModifiersState = ModifiersState::META;
const CMD_SHIFT: ModifiersState = ModifiersState::META.union(ModifiersState::SHIFT);
const CMD_ALT: ModifiersState = ModifiersState::META.union(ModifiersState::ALT);
const CMD_ALT_SHIFT: ModifiersState = CMD_ALT.union(ModifiersState::SHIFT);
const ARROWS: [(Key, Dir); 4] = [
    (Key::ArrowLeft, Dir::Left),
    (Key::ArrowRight, Dir::Right),
    (Key::ArrowUp, Dir::Up),
    (Key::ArrowDown, Dir::Down),
];

pub(crate) const WINDOW: [(&str, ModifiersState, KeyCommand); 17] = [
    ("d", CMD, KeyCommand::Split(Split::Open)),
    ("d", CMD_ALT, KeyCommand::Split(Split::Below)),
    ("∂", CMD_ALT, KeyCommand::Split(Split::Below)),
    ("t", CMD, KeyCommand::Terminal),
    ("w", CMD, KeyCommand::Split(Split::Close)),
    ("k", CMD, KeyCommand::Palette),
    ("p", CMD, KeyCommand::Trigger('@')),
    (",", CMD, KeyCommand::Hotkey(Hotkey::Options)),
    ("n", CMD, KeyCommand::NewSession),
    ("b", CMD, KeyCommand::Sidebar),
    ("d", CMD_SHIFT, KeyCommand::Pane(Pane::Diff)),
    ("u", CMD_SHIFT, KeyCommand::MarkUnread),
    ("p", CMD_SHIFT, KeyCommand::OpenPr),
    (".", CMD, KeyCommand::Interrupt),
    (
        "`",
        ModifiersState::CONTROL,
        KeyCommand::Pane(Pane::Terminal),
    ),
    ("a", CMD_ALT, KeyCommand::Jump(Jump::Needing)),
    ("å", CMD_ALT, KeyCommand::Jump(Jump::Needing)),
];
