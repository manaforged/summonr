use super::*;

impl App {
    pub(in crate::tui::sigil_view) fn peer_rows(&self) -> Vec<FileRow> {
        self.model
            .get()
            .peers
            .iter()
            .map(|agent| FileRow::flat(agent.label().to_string(), agent.label().to_string(), false))
            .collect()
    }

    pub(in crate::tui::sigil_view) fn peer_move(&self, delta: isize) {
        files::shift(&self.peer_cursor, delta, self.model.get().peers.len());
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn peer_enter(&self) {
        let peers = self.model.get().peers.clone();
        let Some(agent) = peers.get(files::pick(&self.peer_cursor, peers.len())) else {
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
