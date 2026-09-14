use std::path::PathBuf;

use sigil::prelude::Cx;
use sigil_tui::{EventRouter, Grid, Host, Screen};
use tokio::sync::mpsc::unbounded_channel;

use super::super::super::{app::App, model::Entry, tests::info, view::root};
use super::Selection;

fn rendered() -> Grid {
    sigil::reactive::Runtime::scope("summonr-selection", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("."), cmd);
        app.resize(40, 12);
        app.model.borrow_mut().entries.push(Entry::Assistant {
            text: "café 日本語\nsecond line\nA👍🏽 e\u{301} 👩\u{200d}💻 🇳🇿B".to_string(),
            done: true,
        });
        app.refresh();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(app)));
        Screen::new(40, 12)
            .frame(&mut host, &mut EventRouter::new(), [], (40, 12))
            .expect("render transcript")
            .clone()
    })
}

#[test]
fn copying_rendered_graphemes_preserves_the_original_text() {
    let mut selected = Selection::new(rendered(), 40, 2, 4);
    selected.move_to(13, 4);
    let expected = "A👍🏽 e\u{301} 👩\u{200d}💻 🇳🇿B";
    assert_eq!(selected.text().as_deref(), Some(expected));
    let mut output = Vec::new();
    selected.paint(&mut output).expect("highlight graphemes");
    assert!(
        String::from_utf8(output)
            .expect("terminal bytes")
            .contains(expected)
    );
}

#[test]
fn drag_copies_unicode_from_the_rendered_cells_in_either_direction() {
    let grid = rendered();
    assert!(grid.text().contains("café 日本語"));
    let mut selected = Selection::new(grid.clone(), 40, 2, 2);
    assert_eq!(
        selected.text(),
        None,
        "a click must not replace the clipboard"
    );
    selected.move_to(12, 2);
    assert_eq!(selected.text().as_deref(), Some("café 日本語"));
    let mut reversed = Selection::new(grid, 40, 12, 2);
    reversed.move_to(2, 2);
    assert_eq!(reversed.text(), selected.text());
}

#[test]
fn multiline_selection_trims_terminal_padding_and_paints_a_highlight() {
    let mut selected = Selection::new(rendered(), 40, 2, 2);
    selected.move_to(12, 3);
    assert_eq!(
        selected.text().as_deref(),
        Some("café 日本語\n  second line")
    );
    let mut output = Vec::new();
    selected.paint(&mut output).expect("paint selection");
    let output = String::from_utf8(output).expect("terminal text");
    assert!(output.contains("\x1b[7m"), "selection is visibly reversed");
    assert!(output.contains("café 日本語"));
    selected.move_to(u16::MAX, u16::MAX);
    assert!(
        selected
            .text()
            .expect("clipped selection")
            .starts_with("café 日本語\n")
    );
}

#[test]
fn multiline_copy_stays_inside_the_selected_pane() {
    let grid = rendered();
    let mut selected = Selection::new(grid.clone(), 7, 2, 2);
    selected.move_to(6, 3);
    assert_eq!(selected.text().as_deref(), Some("café\n  secon"));
    let mut reversed = Selection::new(grid, 7, 6, 3);
    reversed.move_to(2, 2);
    assert_eq!(reversed.text(), selected.text());
}
