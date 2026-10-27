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
        }
    }
}

pub(super) fn bindings() -> Vec<(Shortcut, When, KeyCommand)> {
    use KeyCommand as Do;
    let ctrl_shift = ModifiersState::CONTROL | ModifiersState::SHIFT;
    vec![
        (
            Shortcut::new(LogicalKey::character("y"), ModifiersState::SHIFT),
            When::Persist,
            Do::Always,
        ),
        (Key::F5.into(), When::Free, Do::Retry),
        (
            Shortcut::ctrl(LogicalKey::character("c")),
            When::Unrecorded,
            Do::Interrupt,
        ),
        (
            Shortcut::ctrl(LogicalKey::character("d")),
            When::Idle,
            Do::Quit,
        ),
        (
            LogicalKey::character("t").into(),
            When::DiffPane,
            Do::ToggleRecent,
        ),
        (
            LogicalKey::character("c").into(),
            When::DiffPane,
            Do::CopyPatch,
        ),
        (
            LogicalKey::character("o").into(),
            When::ViewPane,
            Do::ViewExternal,
        ),
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
        (
            LogicalKey::character("a").into(),
            When::HunkOpen,
            Do::HunkAccept,
        ),
        (
            LogicalKey::character("r").into(),
            When::HunkOpen,
            Do::HunkReject,
        ),
        (
            LogicalKey::character("?").into(),
            When::Always,
            Do::Shortcuts,
        ),
        (
            Shortcut::new(LogicalKey::character("?"), ModifiersState::SHIFT),
            When::Always,
            Do::Shortcuts,
        ),
        (
            LogicalKey::character("y").into(),
            When::Always,
            Do::Answer(Reply::Send, GrantAnswer::Allow),
        ),
        (
            LogicalKey::character("n").into(),
            When::Always,
            Do::Answer(Reply::Decline, GrantAnswer::Deny),
        ),
        (Key::Escape.into(), When::Always, Do::Escape),
        (Key::Enter.into(), When::Always, Do::Enter),
        (LogicalKey::character("e").into(), When::Always, Do::Edit),
        (
            Shortcut::ctrl(LogicalKey::character("s")),
            When::Always,
            Do::Save,
        ),
        (LogicalKey::character("n").into(), When::Always, Do::Note),
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
        (LogicalKey::character(" ").into(), When::Always, Do::Toggle),
        (Key::ArrowUp.into(), When::Always, Do::Recall(true)),
        (Key::ArrowDown.into(), When::Always, Do::Recall(false)),
    ]
}
