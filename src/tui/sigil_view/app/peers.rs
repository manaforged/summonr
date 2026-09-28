use super::*;

impl App {
    pub(in crate::tui::sigil_view) fn peer_rows(&self) -> Vec<FileRow> {
        self.model
            .get()
            .peers
            .iter()
            .map(|agent| FileRow {
                path: agent.label().to_string(),
                name: agent.label().to_string(),
                depth: 0,
                dir: false,
                open: false,
            })
            .collect()
    }

    pub(in crate::tui::sigil_view) fn peer_move(&self, delta: isize) {
        let count = self.model.get().peers.len();
        if count == 0 {
            return;
        }
        let cursor = self.peer_cursor.peek().min(count - 1) as isize;
        self.peer_cursor
            .set((cursor + delta).clamp(0, count as isize - 1) as usize);
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn peer_enter(&self) {
        let peers = self.model.get().peers.clone();
        let cursor = self.peer_cursor.peek().min(peers.len().saturating_sub(1));
        let Some(agent) = peers.get(cursor) else {
            return;
        };
        let label = agent.label().to_string();
        drop(peers);
        self.peer_mention(&label);
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
