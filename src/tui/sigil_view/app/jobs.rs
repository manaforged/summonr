use super::*;

impl App {
    pub(in crate::tui::sigil_view) fn job_rows(&self) -> Vec<FileRow> {
        self.model
            .get()
            .jobs
            .iter()
            .map(|(record, _)| {
                let open = self.job_open.peek().as_deref() == Some(record.id.as_str());
                FileRow::flat(record.id.clone(), record.command.clone(), open)
            })
            .collect()
    }

    pub(in crate::tui::sigil_view) fn job_enter(&self) {
        let rows = self.job_rows();
        if let Some(row) = rows.get(files::pick(&self.job_cursor, rows.len())) {
            files::toggle(&self.job_open, &row.path);
            self.refresh();
        }
    }

    pub(in crate::tui::sigil_view) fn job_kill(&self, id: &str) {
        drop(self.cmd.send(Cmd::KillJob(id.to_string())));
        self.refresh();
    }
}
