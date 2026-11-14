use super::*;
use crate::profile::info;
use tokio::sync::mpsc::unbounded_channel;

#[test]
fn arrows_in_the_ledger_leave_the_files_cursor_alone() {
    sigil::reactive::Owner::scope("ledger-arrows", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let (commands, _receiver) = unbounded_channel();
        let app = App::new(info(), workspace.path().to_owned(), commands);
        app.model_mut(|model| model.files = vec!["a.rs".to_string(), "b.rs".to_string()]);
        app.toggle_pane(Pane::Ledger);
        app.pane_move(1);
        assert_eq!(app.pane_list(Pane::Files).at(2), 0);
    });
}

#[test]
fn collapsed_directories_skip_nested_segments_in_file_rows() {
    let items = vec![
        "src/a/one.rs".to_owned(),
        "src/b/two.rs".to_owned(),
        "README.md".to_owned(),
    ];
    let root_only: Vec<(String, bool, bool)> = files::file_rows(&items, &[])
        .into_iter()
        .map(|row| (row.path, row.dir, row.open))
        .collect();
    assert_eq!(
        root_only,
        vec![
            ("README.md".to_owned(), false, true),
            ("src".to_owned(), true, false),
        ]
    );

    let one_level: Vec<(String, usize, bool, bool)> = files::file_rows(&items, &["src".to_owned()])
        .into_iter()
        .map(|row| (row.path, row.depth, row.dir, row.open))
        .collect();
    assert_eq!(
        one_level,
        vec![
            ("README.md".to_owned(), 0, false, true),
            ("src".to_owned(), 0, true, true),
            ("src/a".to_owned(), 1, true, false),
            ("src/b".to_owned(), 1, true, false),
        ]
    );
}

#[test]
fn virtualized_files_pane_renders_and_scrolls_to_cursor() {
    use sigil::Host;
    use sigil::prelude::HostMode;
    use sigil::terminal::Screen;

    sigil::reactive::Owner::scope("files-virtual-list", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let (commands, _receiver) = unbounded_channel();
        let app = App::new(info(), workspace.path().to_owned(), commands);
        app.resize(120, 24);
        app.model_mut(|model| {
            model.files = (0..80).map(|idx| format!("file_{idx:02}.rs")).collect();
        });
        app.toggle_pane(Pane::Files);
        let mut host = Host::new(HostMode::Cell);
        let tree = app.clone();
        host.mount(move || crate::view::root(tree));
        let mut screen = Screen::new(120, 24);
        let mut first = String::new();
        for _ in 0..3 {
            first = screen
                .frame(&mut host, [], (120, 24))
                .expect("frame")
                .text();
        }
        assert!(first.contains("file_00.rs"), "{first}");
        assert!(!first.contains("file_79.rs"), "{first}");

        app.pane_move(79);
        let mut scrolled = String::new();
        for _ in 0..3 {
            scrolled = screen
                .frame(&mut host, [], (120, 24))
                .expect("frame")
                .text();
        }
        assert!(scrolled.contains("file_79.rs"), "{scrolled}");
    });
}
