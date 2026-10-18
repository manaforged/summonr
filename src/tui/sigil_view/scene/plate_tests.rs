use super::tests::splash_screen;
use sigil::prelude::Rgba8;
use sigil::reactive::Runtime;
use sigil::terminal::Ink;

fn splash_grid() -> sigil::terminal::Grid {
    let (_app, mut host, mut screen) = splash_screen(120, 40);
    let mut router = sigil::events::EventRouter::new();
    screen.frame(&mut host, &mut router, [], (120, 40)).unwrap();
    screen
        .frame(&mut host, &mut router, [], (120, 40))
        .unwrap()
        .clone()
}

fn spot(grid: &sigil::terminal::Grid, needle: &str) -> (usize, usize) {
    (0..40)
        .flat_map(|row| (0..120).map(move |col| (col, row)))
        .find(|&(col, row)| grid.get(col, row).is_some_and(|cell| cell.text == needle))
        .unwrap_or_else(|| panic!("{needle} on screen:\n{}", grid.text()))
}

fn glyph(grid: &sigil::terminal::Grid, col: usize, row: usize) -> String {
    grid.get(col, row)
        .map_or_else(String::new, |cell| cell.text.to_string())
}

fn backgrounds(
    grid: &sigil::terminal::Grid,
    cols: std::ops::Range<usize>,
    row: usize,
) -> Vec<Rgba8> {
    cols.filter_map(|col| grid.get(col, row))
        .filter(|cell| cell.text == " ")
        .filter_map(|cell| match cell.bg {
            Ink::Exact(colour) => Some(colour),
            _ => None,
        })
        .collect()
}

#[test]
fn splash_window_frames_the_card_and_the_composer_once() {
    Runtime::scope("splash-window-frame", || {
        let grid = splash_grid();
        let text = grid.text();
        let (_, prompt) = spot(&grid, "❯");
        let left = (0..120)
            .find(|&col| glyph(&grid, col, prompt) == "│")
            .unwrap_or_else(|| panic!("window side on the prompt row:\n{text}"));
        let top = (0..prompt)
            .rev()
            .find(|&row| glyph(&grid, left, row) != "│")
            .unwrap_or_else(|| panic!("window top:\n{text}"));
        let bottom = (prompt..40)
            .find(|&row| glyph(&grid, left, row) != "│")
            .unwrap_or_else(|| panic!("window bottom:\n{text}"));
        assert_eq!(
            [glyph(&grid, left, top), glyph(&grid, left, bottom)],
            ["╭", "╰"],
            "one frame from the card to the composer:\n{text}"
        );
        let session = text
            .lines()
            .position(|line| line.contains("session"))
            .unwrap_or_else(|| panic!("card facts:\n{text}"));
        assert!(
            top < session && session < prompt,
            "facts and prompt share the window:\n{text}"
        );
    });
}

#[test]
fn splash_window_lets_the_sky_show_through_the_card() {
    Runtime::scope("splash-window-card", || {
        let grid = splash_grid();
        let (_, prompt) = spot(&grid, "❯");
        let left = (0..120)
            .find(|&col| glyph(&grid, col, prompt) == "│")
            .unwrap();
        let top = (0..prompt)
            .rev()
            .find(|&row| glyph(&grid, left, row) != "│")
            .unwrap();
        let cells: Vec<_> = (top + 1..prompt.saturating_sub(2))
            .flat_map(|row| backgrounds(&grid, left + 1..left + 100, row))
            .map(|colour| (colour.r, colour.g, colour.b))
            .collect();
        let tints: std::collections::BTreeSet<_> = cells.iter().copied().collect();
        let plate = cells
            .iter()
            .filter(|&&tint| tint == (super::PLATE.r, super::PLATE.g, super::PLATE.b))
            .count();
        assert!(
            plate * 10 < cells.len(),
            "{plate} of {} card cells are opaque plate",
            cells.len()
        );
        assert!(
            tints.len() >= 3,
            "the card shows the sky through its tint: {tints:?}"
        );
    });
}

#[test]
fn splash_window_tints_the_composer_instead_of_covering_the_sky() {
    Runtime::scope("splash-window-composer", || {
        let grid = splash_grid();
        let (col, row) = spot(&grid, "❯");
        let cells: Vec<_> = backgrounds(&grid, col + 2..col + 80, row)
            .into_iter()
            .map(|colour| (colour.r, colour.g, colour.b))
            .collect();
        let plate = cells
            .iter()
            .filter(|&&tint| tint == (super::PLATE.r, super::PLATE.g, super::PLATE.b))
            .count();
        assert!(!cells.is_empty(), "composer cells on the prompt row");
        let left = (0..col)
            .rev()
            .find(|&at| glyph(&grid, at, row) == "│")
            .expect("window side");
        let right = (col..120)
            .find(|&at| glyph(&grid, at, row) == "│")
            .expect("window side");
        let sky: Vec<String> = (row.saturating_sub(1)..row + 3)
            .flat_map(|line| (left + 1..right).map(move |at| (at, line)))
            .map(|(at, line)| glyph(&grid, at, line))
            .filter(|text| {
                text == "▄"
                    || text == "▀"
                    || text
                        .chars()
                        .any(|ch| ('\u{2801}'..='\u{28ff}').contains(&ch))
            })
            .collect();
        assert!(
            sky.is_empty(),
            "sky glyphs inside the composer: {sky:?}\n{}",
            grid.text()
        );
        assert!(
            plate * 2 < cells.len(),
            "{plate} of {} composer cells are opaque plate",
            cells.len()
        );
    });
}

#[test]
fn a_wide_splash_sets_the_wordmark_beside_the_s_in_solid_white_cells() {
    Runtime::scope("splash-lockup", || {
        let (_app, mut host, mut screen) = splash_screen(120, 40);
        let mut router = sigil::events::EventRouter::new();
        screen.frame(&mut host, &mut router, [], (120, 40)).unwrap();
        let grid = screen
            .frame(&mut host, &mut router, [], (120, 40))
            .unwrap()
            .clone();
        let text = grid.text();
        assert!(!text.contains('█'), "{text}");
        let white = sigil::terminal::Ink::Exact(super::palette().text);
        let face = text
            .lines()
            .position(|line| line.contains('┏'))
            .unwrap_or_else(|| panic!("the S on screen:\n{text}"));
        let s_right = text
            .lines()
            .nth(face)
            .and_then(|line| line.chars().position(|glyph| glyph == '┓'))
            .unwrap_or_default();
        let solid = (face..face + 12)
            .flat_map(|row| (0..120).map(move |col| (col, row)))
            .filter(|&(col, row)| {
                col > s_right && grid.get(col, row).is_some_and(|cell| cell.bg == white)
            })
            .count();
        assert!(solid > 60, "wordmark cells beside the S: {solid}\n{text}");
    });
}

#[test]
fn a_wide_splash_centers_the_wordmark_and_facts_on_the_face_of_the_s() {
    Runtime::scope("splash-center", || {
        let (_app, mut host, mut screen) = splash_screen(160, 48);
        let mut router = sigil::events::EventRouter::new();
        screen.frame(&mut host, &mut router, [], (160, 48)).unwrap();
        let grid = screen
            .frame(&mut host, &mut router, [], (160, 48))
            .unwrap()
            .clone();
        let text = grid.text();
        let lines: Vec<&str> = text.lines().collect();
        let face_top = lines.iter().position(|line| line.contains('┏')).unwrap();
        let face_bottom = lines.iter().rposition(|line| line.contains('┗')).unwrap();
        let white = sigil::terminal::Ink::Exact(super::palette().text);
        let word_left = (0..160)
            .find(|&col| {
                (face_top..face_bottom)
                    .any(|row| grid.get(col, row).is_some_and(|cell| cell.bg == white))
            })
            .unwrap_or_else(|| panic!("no wordmark:\n{text}"));
        let inked = |row: usize| {
            grid.get(word_left, row)
                .is_some_and(|cell| cell.bg == white || !cell.text.trim().is_empty())
        };
        let text_top = (0..48)
            .find(|&row| {
                grid.get(word_left, row)
                    .is_some_and(|cell| cell.bg == white)
            })
            .unwrap();
        let text_bottom = (text_top..face_bottom + 2)
            .rev()
            .find(|&row| inked(row))
            .unwrap();
        let offset = (text_top + text_bottom).abs_diff(face_top + face_bottom);
        assert!(
            offset <= 2,
            "text rows {text_top}..={text_bottom}, face rows {face_top}..={face_bottom}\n{text}"
        );
    });
}

#[test]
fn the_splash_glass_reaches_the_window_frame() {
    Runtime::scope("splash-glass-edge", || {
        let (app, _host, _screen) = splash_screen(120, 40);
        let card = super::compose(120, 40, &app).plate.area;
        let window = super::window_area(&app, 120, 40);
        assert_eq!(
            (card[0], card[1], card[2]),
            (window[0], window[1], window[2])
        );
    });
}
