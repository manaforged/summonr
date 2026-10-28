use super::*;

#[derive(Clone, Copy)]
pub(in crate::tui::sigil_view) enum KeyCommand {
    Hotkey(Hotkey),
    Trigger(char),
    NewSession,
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

pub(super) fn bindings() -> Vec<(Shortcut, When, KeyCommand)> {
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
    rows.extend(WINDOW.iter().map(|&(key, command)| {
        (
            Shortcut::new(LogicalKey::character(key), ModifiersState::META),
            When::Window,
            command,
        )
    }));
    rows
}

const WINDOW: [(&str, KeyCommand); 10] = [
    ("k", KeyCommand::Trigger('/')),
    ("p", KeyCommand::Trigger('@')),
    (",", KeyCommand::Hotkey(Hotkey::Options)),
    ("n", KeyCommand::NewSession),
    ("1", KeyCommand::Pane(Pane::Files)),
    ("2", KeyCommand::Pane(Pane::Diff)),
    ("3", KeyCommand::Pane(Pane::Worktrees)),
    ("4", KeyCommand::Pane(Pane::Jobs)),
    ("5", KeyCommand::Pane(Pane::Peers)),
    ("6", KeyCommand::Pane(Pane::Ledger)),
];
