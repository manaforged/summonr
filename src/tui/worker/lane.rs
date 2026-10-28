use super::Cmd;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Lane {
    Read,
    Live,
    AfterTurn,
    Control,
}

impl Cmd {
    pub(super) fn lane(&self) -> Lane {
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
            | Cmd::SetEndpoint(_)
            | Cmd::SetModel(_)
            | Cmd::SetSeat(..)
            | Cmd::SeatRole(..)
            | Cmd::ClearRole(_)
            | Cmd::SaveWorkflow(_)
            | Cmd::ListWorkflows
            | Cmd::SetModule(..)
            | Cmd::SetEffort(_) => Lane::AfterTurn,
            Cmd::Cancel
            | Cmd::Quit
            | Cmd::Tick
            | Cmd::Open(_)
            | Cmd::Mail
            | Cmd::Answer(..)
            | Cmd::LoginAnswer(_) => Lane::Control,
            Cmd::Submit(_)
            | Cmd::Steer(_)
            | Cmd::Queue(_)
            | Cmd::Dequeue
            | Cmd::SteerQueued
            | Cmd::Send(..)
            | Cmd::Export(_)
            | Cmd::Replay
            | Cmd::SetMail(_) => Lane::Live,
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
            | Cmd::View(..)
            | Cmd::Save(..)
            | Cmd::DeleteSession(_) => Lane::Read,
        }
    }
}
