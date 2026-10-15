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

fn distance(a: Rgba8, b: Rgba8) -> u32 {
    [(a.r, b.r), (a.g, b.g), (a.b, b.b)]
        .into_iter()
        .map(|(x, y)| u32::from(x.abs_diff(y)))
        .sum()
}

fn light(clock: Option<u64>) -> Vec<Option<Rgba8>> {
    draw(Mark::Lines, u64::MAX / 2, clock)
        .iter()
        .flatten()
        .map(|glyph| {
            glyph
                .filter(|glyph| {
                    LIGHT.corners.contains(&glyph.text) || LIGHT.straight.contains(&glyph.text)
                })
                .map(|glyph| glyph.fg)
        })
        .collect()
}

#[test]
fn the_face_shimmer_is_faint_moves_and_leaves_the_layers_alone() {
    assert!(lit(None).is_empty());
    let early = lit(Some(5_000));
    let later = lit(Some(5_600));
    assert!(!early.is_empty());
    assert_ne!(early, later);
    let face = palette().text;
    let bound = distance(face, blend(face, palette().accent, 0.4)) + 3;
    for clock in [3_000, 5_000, 9_000] {
        for line in draw(Mark::Lines, u64::MAX / 2, Some(clock)) {
            for glyph in line.into_iter().flatten() {
                if HEAVY.corners.contains(&glyph.text) || HEAVY.straight.contains(&glyph.text) {
                    assert!(distance(glyph.fg, face) <= bound, "{clock}");
                }
            }
        }
        assert_eq!(light(Some(clock)), light(None), "{clock}");
    }
}
