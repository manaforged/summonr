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
fn the_splash_card_covers_the_stars() {
    Runtime::scope("splash-plate", || {
        let workspace = tempfile::tempdir().unwrap();
        std::fs::create_dir(workspace.path().join(".git")).unwrap();
        std::fs::write(workspace.path().join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        let (commands, _receiver) = unbounded_channel();
        let app = crate::tui::sigil_view::app::App::new(
            crate::tui::sigil_view::profile::info(),
            workspace.path().to_owned(),
            commands,
        );
        app.splash_revealed.set(true);
        let cols = 80_u16;
        let rows = 24_u16;
        let mut cells = vec![
            Cell {
                text: "✦".into(),
                fg: Ink::Exact(super::SCRIM),
                bg: Ink::Exact(super::SCRIM),
                ..Cell::blank()
            };
            usize::from(cols) * usize::from(rows)
        ];
        super::super::compose(&mut cells, cols, rows, &app, 0);
        let plate = |cell: &Cell| matches!(cell.bg, Ink::Exact(colour) if colour == super::PLATE);
        let mut bounds: Option<(usize, usize, usize, usize)> = None;
        for row in 0..usize::from(rows) {
            for col in 0..usize::from(cols) {
                let cell = &cells[row * usize::from(cols) + col];
                if !plate(cell) {
                    continue;
                }
                bounds = Some(match bounds {
                    None => (col, row, col, row),
                    Some((x0, y0, x1, y1)) => (x0.min(col), y0.min(row), x1.max(col), y1.max(row)),
                });
            }
        }
        let Some((x0, y0, x1, y1)) = bounds else {
            panic!("splash card missing");
        };
        assert!(x1 > x0 + 1 && y1 > y0 + 1, "splash card is a sliver");
        let edge =
            |cell: &Cell| matches!(cell.fg, Ink::Exact(colour) if colour == super::palette().gray);
        let at = |col: usize, row: usize| &cells[row * usize::from(cols) + col];
        assert_eq!(at(x0, y0).text, "╭", "top left");
        assert_eq!(at(x1, y0).text, "╮", "top right");
        assert_eq!(at(x0, y1).text, "╰", "bottom left");
        assert_eq!(at(x1, y1).text, "╯", "bottom right");
        for col in x0 + 1..x1 {
            assert_eq!(at(col, y0).text, "─", "top edge {col}");
            assert_eq!(at(col, y1).text, "─", "bottom edge {col}");
            assert!(edge(at(col, y0)), "top edge colour {col}");
        }
        for row in y0 + 1..y1 {
            assert_eq!(at(x0, row).text, "│", "left edge {row}");
            assert_eq!(at(x1, row).text, "│", "right edge {row}");
        }
        for row in y0..=y1 {
            for col in x0..=x1 {
                let cell = at(col, row);
                assert!(plate(cell), "card hole at {col},{row} bg={:?}", cell.bg);
                assert_ne!(cell.text, "✦", "star shows through the card at {col},{row}");
            }
        }
    });
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
