use sigil::prelude::Tone;

use super::*;
use crate::jobs::JobState;
use crate::mcp::{McpState, McpTransport, ServerStatus};
use crate::supervise::ServerKind;

impl App {
    pub(in crate::tui::sigil_view) fn file_rows(&self) -> Vec<PaneRow> {
        let model = self.model.get();
        let expanded = self.pane_list(Pane::Files).expanded.get();
        files::file_rows(&model.files, &expanded)
    }

    pub(in crate::tui::sigil_view) fn job_rows(&self) -> Vec<PaneRow> {
        let list = self.pane_list(Pane::Jobs);
        self.model
            .get()
            .jobs
            .iter()
            .map(|(record, status)| {
                let open = list.is_expanded(&record.id);
                PaneRow::flat(record.id.clone(), record.command.clone(), open)
                    .badge(status.label(), job_tone(status.state))
                    .detail(status.text.lines().map(str::to_string))
            })
            .collect()
    }

    pub(in crate::tui::sigil_view) fn mcp_rows(&self) -> Vec<PaneRow> {
        let list = self.pane_list(Pane::Mcp);
        self.model
            .get()
            .mcp
            .iter()
            .map(|server| {
                let open = list.is_expanded(&server.name);
                PaneRow::flat(server.name.clone(), server.name.clone(), open)
                    .status(summary(server))
                    .badge(server.state.label(), mcp_tone(server.state))
                    .note(server.detail.clone())
                    .detail(server.launch.iter().chain(&server.tools).cloned())
            })
            .collect()
    }

    pub(in crate::tui::sigil_view) fn peer_rows(&self) -> Vec<PaneRow> {
        self.model
            .get()
            .peers
            .iter()
            .map(|agent| {
                let tone = if agent.status == crate::mail::Presence::Working {
                    Tone::Info
                } else {
                    Tone::Neutral
                };
                let kind = if agent.kind.is_empty() || agent.kind == crate::mail::KIND {
                    ""
                } else {
                    agent.kind.as_str()
                };
                let dir = agent
                    .cwd
                    .rsplit(['/', '\\'])
                    .find(|part| !part.is_empty())
                    .unwrap_or(&agent.cwd);
                let about = [kind, agent.model.as_str(), dir]
                    .into_iter()
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join(" · ");
                let label = agent.label().to_string();
                PaneRow::flat(label.clone(), format!("@{label}"), false)
                    .status(about)
                    .badge(agent.status.to_string(), tone)
            })
            .collect()
    }

    pub(in crate::tui::sigil_view) fn tree_rows(&self) -> Vec<PaneRow> {
        self.model
            .get()
            .trees
            .iter()
            .map(|row| {
                let (label, tone) = if row.dirty {
                    ("dirty", Tone::Warn)
                } else {
                    ("clean", Tone::Neutral)
                };
                PaneRow::flat(
                    row.tree.path.display().to_string(),
                    row.tree.branch.clone(),
                    false,
                )
                .status(format!("{} ahead", row.ahead))
                .badge(label, tone)
            })
            .collect()
    }

    pub(in crate::tui::sigil_view) fn diff_rows(&self) -> Vec<PaneRow> {
        self.diff_stats()
            .iter()
            .map(|stat| {
                PaneRow::flat(stat.path.clone(), stat.path.clone(), false)
                    .status(format!("+{} −{}", stat.added, stat.removed))
            })
            .collect()
    }

    pub(in crate::tui::sigil_view) fn job_kill(&self, id: &str) {
        drop(self.cmd.send(Cmd::KillJob(id.to_string())));
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn peer_mention(&self, name: &str) {
        let mut draft = self.draft.peek();
        if !draft.is_empty() && !draft.ends_with(' ') {
            draft.push(' ');
        }
        draft.push('@');
        draft.push_str(name);
        draft.push(' ');
        self.replace(draft);
        self.pane_focus.set(false);
        self.refresh();
    }
}

const fn job_tone(state: JobState) -> Tone {
    match state {
        JobState::Running => Tone::Info,
        JobState::Killed => Tone::Warn,
        JobState::Exited(0) => Tone::Success,
        JobState::Exited(_) => Tone::Error,
        JobState::Dead => Tone::Neutral,
    }
}

const fn mcp_tone(state: McpState) -> Tone {
    match state {
        McpState::Live => Tone::Success,
        McpState::Cached | McpState::Unlisted => Tone::Info,
        McpState::Waiting | McpState::Stopped => Tone::Warn,
        McpState::Failed => Tone::Error,
    }
}

fn summary(server: &ServerStatus) -> String {
    let head = match server.kind {
        ServerKind::Lsp => "language server".to_string(),
        ServerKind::Mcp => format!(
            "{} · {} · {}",
            server.transport.map_or("-", McpTransport::label),
            summonr_core::text::plural(server.tools.len(), "tool"),
            server.schema_size(),
        ),
    };
    match server.rss_kb {
        Some(kb) => format!("{head} · mem {}", summonr_core::text::megabytes(kb)),
        None => head,
    }
}
