use sigil::prelude::Icon;

use super::Pane;

pub(super) const NEW: Icon = Icon::Plus;
pub(super) const SEAT: Icon = Icon::Key;
pub(super) const WORKTREE: Icon = Icon::GitPullRequest;

pub(super) const fn pane(pane: Pane) -> Icon {
    match pane {
        Pane::Diff => Icon::FileCode,
        Pane::Worktrees => Icon::GitBranch,
        Pane::Jobs => Icon::Terminal,
        Pane::Peers => Icon::Users,
        Pane::Ledger => Icon::History,
        Pane::Files | Pane::Chat | Pane::Mcp | Pane::View => Icon::Folder,
    }
}
