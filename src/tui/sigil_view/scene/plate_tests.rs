use super::tests::splash_screen;
use sigil::reactive::Runtime;
use sigil::terminal::Ink;

#[test]
fn the_splash_composer_sits_on_the_card_plate() {
    Runtime::scope("splash-composer", || {
        let (_app, mut host, mut screen) = splash_screen(120, 40);
        let mut router = sigil::events::EventRouter::new();
        screen.frame(&mut host, &mut router, [], (120, 40)).unwrap();
        let grid = screen
            .frame(&mut host, &mut router, [], (120, 40))
            .unwrap()
            .clone();
        let (col, row) = (0..40)
            .flat_map(|row| (0..120).map(move |col| (col, row)))
            .find(|&(col, row)| grid.get(col, row).is_some_and(|cell| cell.text == "❯"))
            .expect("prompt on the splash");
        for at in col + 2..col + 30 {
            let cell = grid.get(at, row).expect("composer cell");
            assert!(
                matches!(cell.bg, Ink::Exact(colour) if colour == super::PLATE),
                "composer cell {at},{row} bg={:?}",
                cell.bg
            );
        }
    });
}

#[test]
fn the_splash_card_and_composer_are_aligned_half_cell_plates() {
    Runtime::scope("splash-plates", || {
        let (_app, mut host, mut screen) = splash_screen(120, 40);
        let mut router = sigil::events::EventRouter::new();
        screen.frame(&mut host, &mut router, [], (120, 40)).unwrap();
        let grid = screen
            .frame(&mut host, &mut router, [], (120, 40))
            .unwrap()
            .clone();
        let text = grid.text();
        assert!(!text.contains('╭') && !text.contains('╰'), "{text}");
        let row_of = |needle: &str| {
            text.lines()
                .position(|line| line.contains(needle))
                .unwrap_or_else(|| panic!("{needle} on screen:\n{text}"))
        };
        let edge = |row: usize| {
            (0..120).find(|&col| {
                grid.get(col, row).is_some_and(
                    |cell| matches!(cell.bg, Ink::Exact(colour) if colour == super::PLATE),
                )
            })
        };
        let card = edge(row_of("session"));
        let composer = edge(row_of("❯"));
        assert!(card.is_some(), "card edge:\n{text}");
        assert_eq!(card, composer, "{text}");
        let left = card.unwrap_or_default();
        let filled = |col: usize, row: usize| {
            grid.get(col, row).is_some_and(|cell| {
                matches!(cell.bg, Ink::Exact(colour) if colour == super::PLATE)
                    && !["▄", "▀"].contains(&cell.text.as_str())
            })
        };
        let prompt = row_of("❯");
        let top = (0..prompt)
            .rev()
            .find(|&row| !filled(left + 1, row))
            .map_or(0, |row| row + 1);
        let bottom = (prompt..40)
            .find(|&row| !filled(left + 1, row))
            .unwrap_or(40)
            - 1;
        assert!(
            top + 3 >= prompt && bottom <= prompt + 4,
            "composer rows {top}..{bottom}:\n{text}"
        );
        assert!(
            !filled(left, top) && !filled(left, bottom),
            "composer corners:\n{text}"
        );
        assert!(filled(left, prompt), "composer side:\n{text}");
    });
}
