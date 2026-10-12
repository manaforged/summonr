use sigil::terminal::Ink;

#[test]
fn the_splash_field_covers_the_screen() {
    let cells = super::space(48, 16, 1, super::QUIET);
    assert_eq!(cells.len(), 48 * 16);
    assert!(cells.iter().all(|cell| match cell.bg {
        Ink::Exact(colour) => colour.a == 255,
        _ => false,
    }));
}

#[test]
fn splash_has_at_most_two_bright_points_even_with_a_meteor_on_a_large_screen() {
    for tick in 0..880 {
        let cells = super::space(200, 60, tick, super::QUIET);
        let bright = cells.iter().filter(|cell| cell.text == "✦").count();
        assert!(bright <= 2, "tick {tick}: {bright} simultaneous sparkles");
    }
}

#[test]
fn splash_sky_has_depth_and_more_than_one_star_colour() {
    let cells = super::space(80, 24, 0, super::QUIET);
    let colour = |row: usize, col: usize| match cells[row * 80 + col].bg {
        Ink::Exact(colour) => (colour.r, colour.g, colour.b),
        _ => panic!("bare sky"),
    };
    assert_ne!(colour(0, 40), colour(9, 40), "sky is flat");
    assert_ne!(colour(12, 0), colour(12, 40), "edges are not darker");
    let mut tones = std::collections::BTreeSet::new();
    for cell in &cells {
        if cell.text == "·"
            && let Ink::Exact(colour) = cell.fg
        {
            tones.insert((colour.r, colour.g, colour.b));
        }
    }
    assert!(tones.len() >= 3, "one star colour: {tones:?}");
}

#[test]
fn splash_layers_drift_apart_instead_of_jumping_together() {
    let start = 70;
    let step = super::NEAR_DRIFT;
    assert_eq!(start / super::FAR_DRIFT, (start + step) / super::FAR_DRIFT);
    assert_ne!(
        start / super::NEAR_DRIFT,
        (start + step) / super::NEAR_DRIFT
    );
    let earlier = super::space(80, 24, start, super::QUIET);
    let later = super::space(80, 24, start + step, super::QUIET);
    assert!(earlier.iter().zip(&later).any(|(a, b)| a.text != b.text));
    let uniform = (0..24).all(|row| {
        (0..79).all(|col| later[row * 80 + col].text == earlier[row * 80 + col + 1].text)
    });
    assert!(!uniform, "the whole field still jumps one cell together");
}

#[test]
fn splash_meteor_is_a_fading_streak() {
    let quiet = super::space(96, 28, 200, super::QUIET);
    assert!(quiet.iter().all(|cell| cell.text != "✦"));
    let mut flying = quiet.clone();
    super::meteor(&mut flying, 96, 28, 24, false);
    let streak = flying
        .iter()
        .zip(&quiet)
        .filter(|(after, before)| braille(&after.text) && after != before)
        .count();
    let head = flying.iter().filter(|cell| cell.text == "✦").count();
    assert_eq!(head, 1, "meteor head");
    assert!(streak >= 8, "streak cells {streak}");
}

#[test]
fn splash_skies_differ_and_each_has_a_meteor() {
    let dots = |sky| {
        super::space(80, 24, 0, sky)
            .iter()
            .filter(|cell| cell.text == "·")
            .count()
    };
    assert!(dots(super::DUST) > dots(super::QUIET), "dust is not denser");
    let quiet = super::space(80, 24, 0, super::QUIET);
    let halo = super::space(80, 24, 0, super::HALO);
    assert!(
        quiet.iter().zip(&halo).any(|(a, b)| a.bg != b.bg),
        "halo does not lift a neighbour"
    );
    assert_eq!(
        super::step_sky(super::DUST, 1),
        super::QUIET,
        "cycle does not wrap"
    );
    for sky in [super::QUIET, super::HALO, super::DUST] {
        let flying = super::space(96, 28, 24, sky);
        assert_eq!(
            flying.iter().filter(|cell| cell.text == "✦").count(),
            1,
            "sky {sky} meteor head"
        );
        assert!(
            flying.iter().filter(|cell| braille(&cell.text)).count() >= 8,
            "sky {sky} has no streak"
        );
    }
}

fn braille(text: &str) -> bool {
    text.chars()
        .next()
        .is_some_and(|glyph| ('\u{2800}'..='\u{28ff}').contains(&glyph))
}

#[test]
fn splash_has_one_brief_sparkle_every_two_seconds_with_quiet_gaps() {
    let mut lit = false;
    let mut onsets = Vec::new();
    let mut lit_frames = 0;
    for tick in 120..420 {
        let cells = super::space(200, 60, tick, super::QUIET);
        let bright = cells.iter().filter(|cell| cell.text == "✦").count();
        assert!(bright <= 1, "tick {tick}: {bright} stationary sparkles");
        if bright == 1 {
            lit_frames += 1;
            if !lit {
                onsets.push(tick);
            }
        }
        lit = bright == 1;
    }
    assert_eq!(onsets, [154, 204, 254, 304, 354, 404]);
    assert_eq!(lit_frames, 96);
}

fn luma(cell: &sigil::terminal::Cell) -> u32 {
    match cell.bg {
        Ink::Exact(colour) => u32::from(colour.r) + u32::from(colour.g) + u32::from(colour.b),
        _ => panic!("bare sky"),
    }
}

fn runs(cells: &[sigil::terminal::Cell], cols: u16) -> usize {
    cells
        .chunks(usize::from(cols))
        .map(|row| {
            1 + row
                .windows(2)
                .filter(|pair| {
                    pair[0].fg != pair[1].fg
                        || pair[0].bg != pair[1].bg
                        || pair[0].bold != pair[1].bold
                })
                .count()
        })
        .sum()
}

#[test]
fn the_splash_sky_is_a_galaxy_band_with_a_core_and_a_dust_lane() {
    let (cols, rows) = (120_u16, 36_u16);
    let cells = super::space(cols, rows, 200, super::QUIET);
    let at = |x: u16, y: u16| luma(&cells[usize::from(y) * usize::from(cols) + usize::from(x)]);
    let darkest = cells.iter().map(luma).min().unwrap();
    let brightest = (0..cols)
        .flat_map(|x| (0..rows).map(move |y| (x, y)))
        .max_by_key(|&(x, y)| at(x, y))
        .unwrap();
    assert!(
        brightest.0 > cols / 5
            && brightest.0 < cols - 4
            && brightest.1 > 2
            && brightest.1 < rows - 3,
        "no core inside the sky: brightest at {brightest:?}"
    );
    assert!(
        at(brightest.0, brightest.1) > darkest * 4,
        "the core does not stand out"
    );
    let column: Vec<u32> = (0..rows).map(|y| at(cols / 2, y)).collect();
    let floor = column[0].min(column[usize::from(rows) - 1]);
    let rift = (1..column.len() - 1).any(|mid| {
        let before = column[..mid].iter().max().unwrap();
        let after = column[mid + 1..].iter().max().unwrap();
        column[mid] < *before && column[mid] < *after && *before > floor && *after > floor
    });
    assert!(rift, "no dark lane across the band: {column:?}");
    let dusty = |xs: std::ops::Range<u16>, ys: std::ops::Range<u16>| {
        xs.flat_map(|x| ys.clone().map(move |y| (x, y)))
            .filter(|&(x, y)| {
                braille(&cells[usize::from(y) * usize::from(cols) + usize::from(x)].text)
            })
            .count()
    };
    let (cx, cy) = brightest;
    let core = dusty(cx - 6..cx + 6, cy.saturating_sub(3)..cy + 3);
    let corner = dusty(0..12, 0..6);
    assert!(
        core > corner * 3 && core >= 24,
        "no dust cloud: core {core}, corner {corner}"
    );
}

#[test]
fn the_splash_sky_stays_within_a_run_budget() {
    let (cols, rows) = (200_u16, 60_u16);
    for sky in [super::QUIET, super::HALO, super::DUST] {
        for tick in [0, 24, 160, 300] {
            let cells = super::space(cols, rows, tick, sky);
            let count = runs(&cells, cols);
            assert!(count <= BUDGET, "sky {sky} tick {tick}: {count} runs");
        }
    }
}

const BUDGET: usize = 1_600;
