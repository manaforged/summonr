use super::*;

impl App {
    pub(in crate::tui::sigil_view) fn mcp_rows(&self) -> Vec<FileRow> {
        self.model
            .get()
            .mcp
            .iter()
            .map(|server| {
                let open = self.mcp_open.peek().as_deref() == Some(server.name.as_str());
                FileRow::flat(server.name.clone(), server.name.clone(), open)
            })
            .collect()
    }

    pub(in crate::tui::sigil_view) fn mcp_move(&self, delta: isize) {
        files::shift(&self.mcp_cursor, delta, self.mcp_rows().len());
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn mcp_enter(&self) {
        let rows = self.mcp_rows();
        if let Some(row) = rows.get(files::pick(&self.mcp_cursor, rows.len())) {
            files::toggle(&self.mcp_open, &row.path);
            self.refresh();
        }
    }
}
