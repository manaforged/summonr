use super::*;

impl App {
    pub(in crate::tui::sigil_view) fn job_rows(&self) -> Vec<FileRow> {
        self.model
            .get()
            .jobs
            .iter()
            .map(|(record, _)| FileRow {
                path: record.id.clone(),
                name: record.command.clone(),
                depth: 0,
                dir: false,
                open: self.job_open.peek().as_deref() == Some(record.id.as_str()),
            })
            .collect()
    }

    pub(in crate::tui::sigil_view) fn job_move(&self, delta: isize) {
        let count = self.job_rows().len();
        if count == 0 {
            return;
        }
        let cursor = self.job_cursor.peek().min(count - 1) as isize;
        self.job_cursor
            .set((cursor + delta).clamp(0, count as isize - 1) as usize);
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn job_enter(&self) {
        let rows = self.job_rows();
        let cursor = self.job_cursor.peek().min(rows.len().saturating_sub(1));
        let Some(row) = rows.get(cursor) else {
            return;
        };
        let open = if self.job_open.peek().as_deref() == Some(row.path.as_str()) {
            None
        } else {
            Some(row.path.clone())
        };
        self.job_open.set(open);
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn job_kill(&self, id: &str) {
        drop(self.cmd.send(Cmd::KillJob(id.to_string())));
        self.refresh();
    }
}
