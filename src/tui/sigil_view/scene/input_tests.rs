use sigil::reactive::Runtime;
use sigil::terminal::{Cell, Ink};
use tokio::sync::mpsc::unbounded_channel;

#[test]
fn the_splash_card_is_a_solid_plate() {
    let mut cells = vec![Cell::blank(); 32];
    super::dim_rect(&mut cells, 8, [0, 0, 8, 4], true);
    assert!(cells.iter().all(|cell| matches!(
        cell.bg,
        Ink::Exact(colour) if colour == super::PLATE
    )));
}

#[test]
fn the_splash_caret_sits_after_the_typed_text() {
    Runtime::scope("splash-caret", || {
        let workspace = tempfile::tempdir().unwrap();
        std::fs::create_dir(workspace.path().join(".git")).unwrap();
        std::fs::write(workspace.path().join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        let (commands, _receiver) = unbounded_channel();
        let app = crate::tui::sigil_view::app::App::new(
            crate::tui::sigil_view::profile::info(),
            workspace.path().to_owned(),
            commands,
        );
        app.draft.set("What c".to_string());
        app.caret.set("What c".len());
        let cols = 80_u16;
        let rows = 24_u16;
        let mut cells = vec![Cell::blank(); usize::from(cols) * usize::from(rows)];
        super::super::compose(&mut cells, cols, rows, &app, 0);
        let text: String = cells.iter().map(|cell| cell.text.as_str()).collect();
        let at = text.find("What c").expect("typed text");
        assert_eq!(&text[at..at + "What c│".len()], "What c│");
    });
}
