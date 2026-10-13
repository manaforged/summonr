use sigil::terminal::{Cell, Ink};

fn colour(ink: Ink) -> u32 {
    match ink {
        Ink::Exact(colour) => u32::from(colour.r) + u32::from(colour.g) + u32::from(colour.b),
        _ => panic!("bare sky"),
    }
}

fn braille(text: &str) -> bool {
    text.chars()
        .next()
        .is_some_and(|glyph| ('\u{2800}'..='\u{28ff}').contains(&glyph))
}

fn star(cell: &Cell) -> bool {
    cell.text != " " && cell.text != "▀"
}

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
fn the_splash_sky_is_a_galaxy_band_with_a_core_and_a_dust_lane() {
    let (cols, rows) = (120_u16, 36_u16);
    let cells = super::backdrop(cols, rows, super::QUIET);
    let at = |x: u16, y: u16| &cells[usize::from(y) * usize::from(cols) + usize::from(x)];
    let haze = |x: u16, y: u16| {
        let cell = at(x, y);
        if cell.text == "▀" {
            colour(cell.fg) + colour(cell.bg)
        } else {
            colour(cell.bg) * 2
        }
    };
    assert!(
        cells
            .iter()
            .any(|cell| cell.text == "▀" && cell.fg != cell.bg),
        "the haze has no half-cell detail"
    );
    let darkest = (0..cols)
        .flat_map(|x| (0..rows).map(move |y| (x, y)))
        .map(|(x, y)| haze(x, y))
        .min()
        .unwrap();
    let brightest = (0..cols)
        .flat_map(|x| (0..rows).map(move |y| (x, y)))
        .max_by_key(|&(x, y)| haze(x, y))
        .unwrap();
    assert!(
        brightest.0 > cols / 5
            && brightest.0 < cols - 4
            && brightest.1 > 2
            && brightest.1 < rows - 3,
        "no core inside the sky: brightest at {brightest:?}"
    );
    assert!(
        haze(brightest.0, brightest.1) > darkest * 3,
        "the core does not stand out"
    );
    let column: Vec<u32> = (0..rows).map(|y| haze(cols / 2, y)).collect();
    let floor = column[0].min(column[usize::from(rows) - 1]);
    let rift = (1..column.len() - 1).any(|mid| {
        let before = column[..mid].iter().max().unwrap();
        let after = column[mid + 1..].iter().max().unwrap();
        column[mid] < *before && column[mid] < *after && *before > floor && *after > floor
    });
    assert!(rift, "no dark lane across the band: {column:?}");
    let stars = |xs: std::ops::Range<u16>, ys: std::ops::Range<u16>| {
        xs.flat_map(|x| ys.clone().map(move |y| (x, y)))
            .filter(|&(x, y)| star(at(x, y)))
            .count()
    };
    let (cx, cy) = brightest;
    let core = stars(cx.saturating_sub(12)..cx + 4, cy.saturating_sub(4)..cy + 4);
    let corner = stars(0..12, 0..6);
    assert!(
        core > corner * 2 && core >= 6,
        "no star cloud: core {core}, corner {corner}"
    );
}

#[test]
fn splash_stars_move_a_fraction_of_a_cell_every_tick() {
    let (cols, rows) = (120_u16, 36_u16);
    let frame = |tick| super::space(cols, rows, tick, super::QUIET);
    for tick in 100..112 {
        let now = frame(tick);
        let next = frame(tick + 1);
        let moved = now.iter().zip(&next).filter(|(a, b)| a != b).count();
        assert!(moved > 0, "tick {tick}: nothing moved");
        let nudged = now
            .iter()
            .zip(&next)
            .filter(|(a, b)| braille(&a.text) && braille(&b.text) && a.text != b.text)
            .count();
        assert!(nudged > 0, "tick {tick}: no star moved within its cell");
    }
}

#[test]
fn a_splash_tick_redraws_only_its_moving_cells() {
    let (cols, rows) = (200_u16, 60_u16);
    for sky in [super::QUIET, super::HALO, super::DUST] {
        for tick in (0..880).step_by(7) {
            let spots = super::moving(cols, rows, tick, sky).len();
            assert!(spots <= 600, "sky {sky} tick {tick}: {spots} cells redrawn");
        }
    }
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
fn splash_meteor_is_a_fading_streak() {
    let head = |tick| {
        super::moving(96, 28, tick, super::QUIET)
            .iter()
            .filter(|spot| spot.cell.text == "✦" && spot.cell.bold)
            .count()
    };
    assert_eq!(head(200), 0, "a meteor outside its flight");
    assert_eq!(head(24), 1, "meteor head");
    let streak = super::moving(96, 28, 24, super::QUIET).len()
        - super::flying(96, 28, 24, super::QUIET).len();
    assert!(streak >= 8, "streak cells {streak}");
}

#[test]
fn splash_skies_differ_and_each_has_a_meteor() {
    let stars = |sky| super::moving(80, 24, 0, sky).len();
    assert!(
        stars(super::DUST) > stars(super::QUIET),
        "dust is not denser"
    );
    assert_ne!(
        super::space(80, 24, 30, super::QUIET),
        super::space(80, 24, 30, super::HALO),
        "halo looks like quiet"
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
    }
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

#[test]
fn the_galaxy_haze_mixes_cosmic_hues() {
    let cells = super::backdrop(120, 36, super::QUIET);
    let colours: Vec<(i32, i32, i32)> = cells
        .iter()
        .flat_map(|cell| [cell.fg, cell.bg])
        .filter_map(|ink| match ink {
            Ink::Exact(colour) => Some((
                i32::from(colour.r),
                i32::from(colour.g),
                i32::from(colour.b),
            )),
            _ => None,
        })
        .collect();
    let teal = colours.iter().any(|&(r, g, b)| g > r + 10 && b > r + 10);
    let rose = colours.iter().any(|&(r, g, b)| r > g + 20 && r > b);
    let gold = colours.iter().any(|&(r, g, b)| r > b + 30 && g > b);
    assert!(
        teal && rose && gold,
        "teal {teal}, rose {rose}, gold {gold}"
    );
}
