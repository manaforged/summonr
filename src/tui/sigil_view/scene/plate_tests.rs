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
        let card = super::compose(120, 40, &app, 0).plate.area;
        let window = super::window_area(&app, 120, 40);
        assert_eq!(
            (card[0], card[1], card[2]),
            (window[0], window[1], window[2])
        );
    });
}

#[test]
fn the_splash_card_is_two_rows_taller_than_its_body() {
    Runtime::scope("splash-card-height", || {
        let (cols, rows) = (160, 120);
        let (app, _host, _screen) = splash_screen(cols, rows);
        let facts = super::facts(&app.model.get(), &app);
        let card = super::fitting(cols, rows, &facts);
        let [_, body_rows] = super::body(cols, card, &facts);
        assert!(body_rows > 2);
        assert_eq!(
            super::compose(cols, rows, &app, 0).plate.area[3],
            body_rows + 2
        );
    });
}

#[test]
fn the_wordmark_overlaps_the_s_and_facts_fade_during_the_final_settle() {
    Runtime::scope("splash-wordmark-sequence", || {
        let (app, _host, _screen) = splash_screen(120, 40);
        let settled = super::compose(120, 40, &app, 0).plate;
        let white = sigil::terminal::Ink::Exact(super::palette().text);
        let width = usize::from(settled.area[2]);
        let first = settled
            .cells
            .iter()
            .position(|cell| cell.bg == white)
            .expect("settled wordmark has solid text cells");
        let x = settled.area[0] + (first % width) as u16;
        let y = settled.area[1] + (first / width) as u16;
        app.splash_revealed.set(false);
        let at = |millis| super::compose(120, 40, &app, millis).plate;
        let early = at(460);
        assert!(early.cells.iter().any(|cell| cell.text == "┌"));
        assert!(!early.cells.iter().any(|cell| cell.text == "┏"));
        assert_eq!(
            early.at([x, y]).expect("wordmark origin").bg,
            early.cells[0].bg
        );
        let overlap = at(520);
        assert!(!overlap.cells.iter().any(|cell| cell.text == "┏"));
        let faint = overlap.at([x, y]).expect("fading wordmark").bg;
        let brighter = at(560).at([x, y]).expect("fading wordmark").bg;
        assert_ne!(faint, early.cells[0].bg);
        assert_ne!(faint, brighter, "wordmark must fade rather than pop in");
        let middle = at(760);
        let left = middle.at([x, y]).expect("leading column").bg;
        let edge = middle.at([x + 28, y]).expect("soft edge").bg;
        assert_ne!(left, edge);
        assert_ne!(edge, middle.cells[0].bg);
        assert_eq!(
            middle.at([x + 56, y]).expect("unrevealed column").bg,
            middle.cells[0].bg
        );
        assert_eq!(
            middle.at([x + 28, y + 1]).expect("next wordmark row").bg,
            edge,
            "the fade must stay aligned across rows"
        );
        assert!(!middle.cells.iter().any(|cell| cell.text == "w"));
        let workdir = settled
            .cells
            .iter()
            .position(|cell| cell.text == "w")
            .expect("workdir label");
        assert!(!at(1_100).cells.iter().any(|cell| cell.text == "w"));
        let fading = at(1_200);
        assert_eq!(fading.cells[workdir].text, "w");
        assert_ne!(fading.cells[workdir].fg, settled.cells[workdir].fg);
        assert_eq!(at(1_280), settled);
        assert_eq!(super::reveal_ms(&app), 1_280);
        assert!(super::compose(120, 40, &app, 1_200).shimmer.is_none());
        assert!(super::compose(120, 40, &app, 1_280).shimmer.is_some());
    });
}

#[test]
fn compact_cards_skip_the_beats_for_elements_they_do_not_show() {
    Runtime::scope("splash-compact-sequence", || {
        for (cols, rows, facts_start, end) in [(60, 40, 600, 760), (120, 20, 640, 800)] {
            let (app, _host, _screen) = splash_screen(cols, rows);
            let settled = super::compose(cols, rows, &app, 0).plate;
            let workdir = settled
                .cells
                .iter()
                .position(|cell| cell.text == "w")
                .expect("workdir label");
            app.splash_revealed.set(false);
            app.splash_since
                .set(std::time::Instant::now() - std::time::Duration::from_secs(10));
            let at = |millis| super::compose(cols, rows, &app, millis).plate;
            assert!(!at(facts_start).cells.iter().any(|cell| cell.text == "w"));
            let fading = at(facts_start + 80);
            assert_eq!(fading.cells[workdir].text, "w");
            assert_ne!(fading.cells[workdir].fg, settled.cells[workdir].fg);
            assert_eq!(at(end), settled, "{cols}x{rows}");
            assert_eq!(super::reveal_ms(&app), end);
        }
    });
}

#[test]
fn typing_or_disabling_animations_skips_the_whole_splash_sequence() {
    Runtime::scope("splash-reveal-skip", || {
        let (app, _host, _screen) = splash_screen(120, 40);
        let settled = super::compose(120, 40, &app, 0).plate;
        app.splash_revealed.set(false);
        app.draft.set("hello".to_string());
        assert_eq!(super::compose(120, 40, &app, 0).plate, settled);
        app.draft.set(String::new());
        app.model_mut(|model| model.prefs.animations = false);
        assert_eq!(super::compose(120, 40, &app, 0).plate, settled);
    });
}
