use super::super::pane::Pane;
use super::screens;
use crate::tui::Msg;

#[test]
fn arrow_down_in_the_view_pane_scrolls_the_file() {
    let text: String = (1..=80).map(|n| format!("line number {n}\n")).collect();
    let frames = screens(
        120,
        30,
        |app| {
            app.view_open("notes.txt".to_owned());
            app.update(Msg::View {
                path: "notes.txt".to_owned(),
                file: std::path::PathBuf::from("notes.txt"),
                text: text.clone(),
                added: Vec::new(),
                image: None,
            });
            assert_eq!(app.layout.peek().secondary, Some(Pane::View));
        },
        &[&|app| app.view_move(20)],
    );
    let shows = |frame: &str, n: usize| {
        let want = format!("line number {n}");
        frame.lines().any(|line| line.trim_end().ends_with(&want))
    };
    assert!(shows(&frames[0], 2), "{}", frames[0]);
    assert!(!shows(&frames[1], 2), "{}", frames[1]);
    assert!(shows(&frames[1], 22), "{}", frames[1]);
}
