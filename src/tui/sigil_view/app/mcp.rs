use super::*;

impl App {
    pub(in crate::tui::sigil_view) fn mcp_rows(&self) -> Vec<FileRow> {
        self.model
            .get()
            .mcp
            .iter()
            .map(|server| FileRow {
                path: server.name.clone(),
                name: server.name.clone(),
                depth: 0,
                dir: false,
                open: self.mcp_open.peek().as_deref() == Some(server.name.as_str()),
            })
            .collect()
    }

    pub(in crate::tui::sigil_view) fn mcp_move(&self, delta: isize) {
        let count = self.mcp_rows().len();
        if count == 0 {
            return;
        }
        let cursor = self.mcp_cursor.peek().min(count - 1) as isize;
        self.mcp_cursor
            .set((cursor + delta).clamp(0, count as isize - 1) as usize);
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn mcp_enter(&self) {
        let rows = self.mcp_rows();
        let cursor = self.mcp_cursor.peek().min(rows.len().saturating_sub(1));
        let Some(row) = rows.get(cursor) else {
            return;
        };
        let open = if self.mcp_open.peek().as_deref() == Some(row.path.as_str()) {
            None
        } else {
            Some(row.path.clone())
        };
        self.mcp_open.set(open);
        self.refresh();
    }
}
