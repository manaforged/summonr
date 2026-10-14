fn braille(text: &str) -> bool {
    text.chars()
        .next()
        .is_some_and(|glyph| ('\u{2800}'..='\u{28ff}').contains(&glyph))
}

fn star(text: &str) -> bool {
    braille(text) || text == "·" || text == "•"
}

#[test]
fn splash_stars_hold_still_and_only_twinkle() {
    let (cols, rows) = (200_u16, 60_u16);
    let frame = |tick| super::space(cols, rows, tick, 0);
    let first = frame(125);
    let places = |cells: &[super::Cell]| {
        cells
            .iter()
            .enumerate()
            .filter(|(_, cell)| star(&cell.text))
            .map(|(at, cell)| (at, cell.text.clone()))
            .collect::<Vec<_>>()
    };
    let mut twinkled = 0;
    for tick in [126, 145, 175, 225, 325] {
        let later = frame(tick);
        assert_eq!(places(&first), places(&later), "stars moved by tick {tick}");
        twinkled += first.iter().zip(&later).filter(|(a, b)| a.fg != b.fg).count();
    }
    assert!(places(&first).len() >= 300, "only {} stars", places(&first).len());
    assert!(twinkled > 0, "no star twinkled");
}

#[test]
fn a_splash_tick_redraws_only_its_moving_cells() {
    let (cols, rows) = (200_u16, 60_u16);
    for sky in [0, 1, 2] {
        for tick in (0..880).step_by(7) {
            let spots = super::moving(cols, rows, tick as f64, sky).len();
            assert!(spots <= 600, "sky {sky} tick {tick}: {spots} cells redrawn");
        }
    }
}

#[test]
fn splash_has_at_most_two_bright_points_even_with_a_meteor_on_a_large_screen() {
    for tick in 0..880 {
        let cells = super::space(200, 60, tick, 0);
        let bright = cells.iter().filter(|cell| cell.text == "✦").count();
        assert!(bright <= 2, "tick {tick}: {bright} simultaneous sparkles");
    }
}

#[test]
fn splash_meteor_is_a_fading_streak() {
    let head = |tick| {
        super::moving(96, 28, tick as f64, 0)
            .iter()
            .filter(|spot| spot.cell.text == "✦" && spot.cell.bold)
            .count()
    };
    assert_eq!(head(200), 0, "a meteor outside its flight");
    assert_eq!(head(24), 1, "meteor head");
    let streak = super::moving(96, 28, 24.0, 0).len() - super::twinkling(96, 28, 24.0, 0).len();
    assert!(streak >= 8, "streak cells {streak}");
}

#[test]
fn each_launch_draws_a_different_galaxy() {
    let one = super::backdrop(120, 36, 1);
    let two = super::backdrop(120, 36, 2);
    let differ = one.iter().zip(&two).filter(|(a, b)| a != b).count();
    assert!(
        differ > 120 * 36 / 4,
        "two launches share most of the sky: {differ} cells differ"
    );
    for seed in [1, 2, 3] {
        let flying = super::space(96, 28, 24, seed);
        assert_eq!(
            flying.iter().filter(|cell| cell.text == "✦").count(),
            1,
            "launch {seed} has no meteor head"
        );
    }
}

#[test]
fn splash_has_one_brief_sparkle_every_two_seconds_with_quiet_gaps() {
    let mut lit = false;
    let mut onsets = Vec::new();
    let mut lit_frames = 0;
    for tick in 120..420 {
        let cells = super::space(200, 60, tick, 0);
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
fn the_stars_crowd_the_band_and_range_in_brightness() {
    let (cols, rows) = (223_u16, 67_u16);
    for seed in 0..4 {
        let haze = super::backdrop(cols, rows, seed);
        let sky = super::space(cols, rows, 300, seed);
        let (mut band, mut open) = ([0_u32; 2], [0_u32; 2]);
        for (under, cell) in haze.iter().zip(&sky) {
            let glow = (lit(under.fg) + lit(under.bg)) / 2.0;
            let tally = if glow > 150.0 {
                &mut band
            } else if glow < 40.0 {
                &mut open
            } else {
                continue;
            };
            tally[0] += 1;
            tally[1] += u32::from(star(&cell.text));
        }
        let density = |[cells, stars]: [u32; 2]| stars as f32 / cells.max(1) as f32;
        assert!(
            density(band) >= 1.4 * density(open),
            "sky {seed}: {:.3} stars per band cell against {:.3} in open sky",
            density(band),
            density(open)
        );
        let count = |glyph: &str| sky.iter().filter(|cell| cell.text == glyph).count();
        let faint = sky.iter().filter(|cell| braille(&cell.text)).count();
        let bright = count("·") + count("•");
        assert!(
            faint >= 5 * bright && bright >= 10 && count("•") >= 1,
            "sky {seed}: {faint} faint, {} bright, {} brilliant stars",
            count("·"),
            count("•")
        );
    }
}

fn lit(ink: sigil::terminal::Ink) -> f32 {
    match ink {
        sigil::terminal::Ink::Exact(c) => f32::from(c.r) + f32::from(c.g) + f32::from(c.b),
        _ => 0.0,
    }
}

#[test]
fn the_galaxy_dust_paints_in_few_colours_and_long_runs() {
    let (cols, rows) = (223_u16, 67_u16);
    for seed in 0..10 {
        let cells = super::backdrop(cols, rows, seed);
        let shades: std::collections::BTreeSet<String> = cells
            .iter()
            .flat_map(|cell| [format!("{:?}", cell.fg), format!("{:?}", cell.bg)])
            .collect();
        assert!(
            (6..=40).contains(&shades.len()),
            "sky {seed}: the haze uses {} colours",
            shades.len()
        );
        let changes: usize = cells
            .chunks(usize::from(cols))
            .map(|row| row.windows(2).filter(|pair| pair[0] != pair[1]).count())
            .sum();
        let per_row = changes / usize::from(rows);
        assert!(
            per_row <= 40,
            "sky {seed}: {per_row} colour changes per row"
        );
    }
    let stars = super::space(cols, rows, 30, 0)
        .iter()
        .filter(|cell| braille(&cell.text))
        .count();
    assert!(stars >= 150, "only {stars} stars");
}

#[test]
fn the_galaxy_dust_streaks_along_the_band() {
    let (cols, rows, window) = (223_usize, 67_usize, 12_usize);
    let mut total = 0.0;
    for seed in 0..9 {
        let cells = super::backdrop(cols as u16, rows as u16, seed);
        let light = |x: usize, y: usize| {
            let cell = &cells[y / 2 * cols + x];
            lit(if y.is_multiple_of(2) {
                cell.fg
            } else {
                cell.bg
            })
        };
        let (mut sum, mut weight) = (0.0_f32, 0.0_f32);
        for top in (1..rows * 2 - window).step_by(window) {
            for left in (1..cols - window).step_by(window) {
                let (mut xx, mut xy, mut yy) = (0.0_f32, 0.0_f32, 0.0_f32);
                for y in top..top + window {
                    for x in left..left + window {
                        if light(x, y) <= 40.0 {
                            continue;
                        }
                        let gx = (light(x + 1, y) - light(x - 1, y)) / 2.0;
                        let gy = (light(x, y + 1) - light(x, y - 1)) / 2.0;
                        (xx, xy, yy) = (xx + gx * gx, xy + gx * gy, yy + gy * gy);
                    }
                }
                if xx + yy > 0.0 {
                    let coherence =
                        (((xx - yy).powi(2) + 4.0 * xy * xy).sqrt() / (xx + yy)).powi(2);
                    (sum, weight) = (sum + coherence * (xx + yy), weight + xx + yy);
                }
            }
        }
        total += sum / weight;
    }
    let coherence = total / 9.0;
    assert!(
        coherence >= 0.3,
        "dust coherence {coherence:.3}, the haze reads as round blobs"
    );
}

#[test]
fn the_galaxy_band_shifts_hue_along_its_length() {
    for seed in 0..8 {
        let (mut lit, mut blue, mut plum) = (0, 0, 0);
        for cell in super::backdrop(223, 67, seed) {
            for ink in [cell.fg, cell.bg] {
                let sigil::terminal::Ink::Exact(c) = ink else {
                    continue;
                };
                let (r, g, b) = (u32::from(c.r), u32::from(c.g), u32::from(c.b));
                if r + g + b <= 90 {
                    continue;
                }
                lit += 1;
                blue += usize::from(g >= r + 8);
                plum += usize::from(r >= g + 10 && b > r);
            }
        }
        assert!(
            blue * 25 >= lit,
            "sky {seed}: {blue} of {lit} lit half cells are blue"
        );
        assert!(
            plum * 10 >= lit,
            "sky {seed}: {plum} of {lit} lit half cells are plum"
        );
    }
}

#[test]
fn every_launch_spreads_dust_across_the_frame() {
    let (cols, rows) = (223_usize, 67_usize);
    for seed in 0..10 {
        let cells = super::backdrop(cols as u16, rows as u16, seed);
        let shows = |x: usize, y: usize| {
            let cell = &cells[y * cols + x];
            lit(cell.fg) > 90.0 || lit(cell.bg) > 90.0
        };
        let sides = [
            (0..rows).filter(|y| shows(0, *y)).count(),
            (0..rows).filter(|y| shows(cols - 1, *y)).count(),
            (0..cols).filter(|x| shows(*x, 0)).count(),
            (0..cols).filter(|x| shows(*x, rows - 1)).count(),
        ];
        let reached = sides.iter().filter(|count| **count >= 2).count();
        assert!(
            reached >= 2,
            "sky {seed}: dust reaches {reached} frame edges {sides:?}, a whole galaxy sits inside"
        );
    }
}

#[test]
fn the_band_carries_nebula_clouds() {
    let cells = super::backdrop(223, 67, 5);
    let rose = cells
        .iter()
        .flat_map(|cell| [cell.fg, cell.bg])
        .filter(|ink| matches!(ink, sigil::terminal::Ink::Exact(colour) if colour.r > colour.b + 4))
        .count();
    assert!(rose > 100, "only {rose} half cells carry the nebula tint");
}
