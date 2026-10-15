use super::*;

const MOCKUP: [&str; 19] = [
    "          ┌──────────────────────────┐",
    "        ┌──────────────────────────┐ │",
    "      ┌──────────────────────────┐ │ │",
    "    ┌──────────────────────────┐ │ │─┘",
    "  ┌──────────────────────────┐ │ │─┘",
    "┏━━━━━━━━━━━━━━━━━━━━━━━━━━┓ │ │─┘───┐",
    "┃                          ┃ │─┘───┐ │",
    "┃                          ┃─┘───┐ │ │",
    "┃    ┏━━━━━━━━━━━━━━━━━━━━━┛───┐ │ │ │",
    "┃    ┃ └─────────────────────┐ │ │ │ │",
    "┃    ┗━━━━━━━━━━━━━━━━━━━━━┓ │ │ │ │ │",
    "┃                          ┃ │ │ │ │ │",
    "┃                          ┃ │ │ │ │ │",
    "┗━━━━━━━━━━━━━━━━━━━━━┓    ┃ │ │ │ │─┘",
    "  ┌───────────────────┃    ┃ │ │ │─┘",
    "┏━━━━━━━━━━━━━━━━━━━━━┛    ┃ │ │─┘",
    "┃                          ┃ │─┘",
    "┃                          ┃─┘",
    "┗━━━━━━━━━━━━━━━━━━━━━━━━━━┛",
];

#[test]
fn the_line_stack_matches_the_terminal_s_mockup() {
    let drawn: Vec<String> = draw(Mark::Lines, u64::MAX / 2, None)
        .iter()
        .map(|row| {
            row.iter()
                .map(|glyph| glyph.map_or(' ', |glyph| glyph.text))
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect();
    assert_eq!(drawn, MOCKUP);
}

fn lit(clock: Option<u64>) -> Vec<(usize, usize)> {
    let face = palette().text;
    draw(Mark::Lines, u64::MAX / 2, clock)
        .iter()
        .enumerate()
        .flat_map(|(row, line)| {
            line.iter().enumerate().filter_map(move |(col, glyph)| {
                glyph
                    .filter(|glyph| {
                        HEAVY.corners.contains(&glyph.text) || HEAVY.straight.contains(&glyph.text)
                    })
                    .filter(|glyph| glyph.fg != face)
                    .map(|_| (row, col))
            })
        })
        .collect()
}

#[test]
fn the_orbit_dash_circles_the_face_and_stays_off_without_motion() {
    assert!(lit(None).is_empty());
    let early = lit(Some(5_000));
    let later = lit(Some(5_400));
    assert!(!early.is_empty());
    assert!(!later.is_empty());
    assert_ne!(early, later);
}
