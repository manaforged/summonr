use super::tests::splash_screen;
use sigil::reactive::Runtime;

#[test]
fn the_splash_composer_has_the_cards_rounded_frame() {
    Runtime::scope("splash-frames", || {
        let (_app, mut host, mut screen) = splash_screen(120, 40);
        let mut router = sigil::events::EventRouter::new();
        screen.frame(&mut host, &mut router, [], (120, 40)).unwrap();
        let grid = screen
            .frame(&mut host, &mut router, [], (120, 40))
            .unwrap()
            .clone();
        let text = grid.text();
        let lines: Vec<Vec<char>> = text.lines().map(|line| line.chars().collect()).collect();
        let at = |col: usize, row: usize| lines.get(row).and_then(|line| line.get(col)).copied();
        let row_of = |needle: &str| {
            text.lines()
                .position(|line| line.contains(needle))
                .unwrap_or_else(|| panic!("{needle} on screen:\n{text}"))
        };
        let card = row_of("session");
        assert!(lines[card].contains(&'│'), "card frame:\n{text}");
        let prompt = row_of("❯");
        let left = lines[prompt]
            .iter()
            .position(|glyph| *glyph == '│')
            .unwrap_or_else(|| panic!("composer frame:\n{text}"));
        let right = lines[prompt]
            .iter()
            .rposition(|glyph| *glyph == '│')
            .unwrap_or_default();
        let top = (0..prompt)
            .rev()
            .find(|&row| at(left, row) != Some('│'))
            .unwrap_or_else(|| panic!("composer top:\n{text}"));
        let bottom = (prompt..lines.len())
            .find(|&row| at(left, row) != Some('│'))
            .unwrap_or_else(|| panic!("composer bottom:\n{text}"));
        assert_eq!(
            [
                at(left, top),
                at(right, top),
                at(left, bottom),
                at(right, bottom)
            ],
            [Some('╭'), Some('╮'), Some('╰'), Some('╯')],
            "composer corners:\n{text}"
        );
        assert!(top > card, "{text}");
    });
}
