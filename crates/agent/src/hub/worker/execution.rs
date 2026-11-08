use super::Cmd;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ExecutionClass {
    Read,
    Live,
    AfterTurn,
    Control,
}

impl Cmd {
    pub(super) fn execution_class(&self) -> ExecutionClass {
        match self {
            Cmd::Command(_)
            | Cmd::Branch { .. }
            | Cmd::Login(_)
            | Cmd::SetKey(..)
            | Cmd::Logout(_)
            | Cmd::Merge(_)
            | Cmd::Discard(_)
            | Cmd::SetVerify(_)
            | Cmd::ToggleApproval
            | Cmd::SetModel(_)
            | Cmd::SelectModel(..)
            | Cmd::AssignModel(..)
            | Cmd::ClearRole(_)
            | Cmd::SaveWorkflow(_)
            | Cmd::ListWorkflows
            | Cmd::SetModule(..)
            | Cmd::SetChrome(_)
            | Cmd::SetEffort(_) => ExecutionClass::AfterTurn,
            Cmd::Forge(crate::forge::Ask::Status) => ExecutionClass::Read,
            Cmd::Forge(_) => ExecutionClass::AfterTurn,
            Cmd::Cancel
            | Cmd::CancelLogin
            | Cmd::Quit
            | Cmd::Tick
            | Cmd::Peers(_)
            | Cmd::Open(_)
            | Cmd::TakeOver(_)
            | Cmd::Attach(_)
            | Cmd::Detach
            | Cmd::Mail
            | Cmd::Answer(..)
            | Cmd::LoginAnswer(_) => ExecutionClass::Control,
            Cmd::Submit(_)
            | Cmd::Steer(_)
            | Cmd::Queue(_)
            | Cmd::Dequeue
            | Cmd::SteerQueued
            | Cmd::Send(..)
            | Cmd::Export(_)
            | Cmd::Replay
            | Cmd::SetMail(_) => ExecutionClass::Live,
            Cmd::ListModels { .. }
            | Cmd::Pin(..)
            | Cmd::SaveDefault(_)
            | Cmd::ListSessions
            | Cmd::ListUsage
            | Cmd::ListPrompts(_)
            | Cmd::ListFiles
            | Cmd::Jobs
            | Cmd::Mcp
            | Cmd::Offers
            | Cmd::KillJob(_)
            | Cmd::Clipboard
            | Cmd::Diff(_)
            | Cmd::SessionDiff(_)
            | Cmd::Patch(..)
            | Cmd::Review { .. }
            | Cmd::Trees
            | Cmd::ListPanels
            | Cmd::RunPanel(_)
            | Cmd::ActPanel { .. }
            | Cmd::View(..)
            | Cmd::Save(..)
            | Cmd::DeleteSession(_)
            | Cmd::Stop(_)
            | Cmd::MarkUnread(_)
            | Cmd::Rename { .. } => ExecutionClass::Read,
        }
    }
}
