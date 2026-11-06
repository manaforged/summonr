use super::super::app::App;
use super::super::ui::Tab;
use super::{State, busy_info, frame_with};

fn dialog(cols: u16, rows: u16, tab: Tab) -> String {
    frame_with(busy_info(), cols, rows, State::Idle, |app: &App| {
        app.replace("/model".to_owned());
        app.submit();
        app.show_tab(tab);
    })
    .join("\n")
}

fn tab_row(screen: &str) -> &str {
    screen
        .lines()
        .find(|line| line.contains("Models") && line.contains("Providers"))
        .unwrap_or_else(|| panic!("no tab row:\n{screen}"))
}

#[test]
fn every_settings_dialog_names_all_three_tabs() {
    for tab in [Tab::Models, Tab::Providers, Tab::Usage] {
        for (cols, rows) in [(80, 30), (120, 40), (160, 50)] {
            let screen = dialog(cols, rows, tab);
            let row = tab_row(&screen);
            assert!(row.contains("Usage"), "{tab:?} {cols}: {row}");
        }
    }
}

#[test]
fn the_tab_row_keeps_the_labels_left_and_the_hint_after_them() {
    let screen = dialog(120, 40, Tab::Models);
    let row = tab_row(&screen);
    let models = row.find("Models").expect("models");
    let providers = row.find("Providers").expect("providers");
    let usage = row.find("Usage").expect("usage");
    assert!(models < providers && providers < usage, "{row}");
    if let Some(hint) = row.find("←/→") {
        assert!(usage < hint, "{row}");
    }
}
