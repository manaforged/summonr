use super::*;
use crate::tui::sigil_view::profile::info;
use sigil::reactive::Runtime;
use tokio::sync::mpsc::unbounded_channel;

#[test]
fn arrows_in_the_ledger_leave_the_files_cursor_alone() {
    Runtime::scope("ledger-arrows", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let (commands, _receiver) = unbounded_channel();
        let app = App::new(info(), workspace.path().to_owned(), commands);
        app.model_mut(|model| model.files = vec!["a.rs".to_string(), "b.rs".to_string()]);
        app.toggle_pane(Pane::Ledger);
        app.pane_move(1);
        assert_eq!(app.pane_list(Pane::Files).at(2), 0);
    });
}
