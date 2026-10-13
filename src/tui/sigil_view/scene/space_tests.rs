fn braille(text: &str) -> bool {
    text.chars()
        .next()
        .is_some_and(|glyph| ('\u{2800}'..='\u{28ff}').contains(&glyph))
}

#[test]
fn splash_stars_move_a_fraction_of_a_cell_every_tick() {
    let (cols, rows) = (200_u16, 60_u16);
    let frame = |tick| super::space(cols, rows, tick, 0);
    let mut ticks_with_motion = 0;
    let mut nudged = 0;
    for tick in 100..112 {
        let now = frame(tick);
        let next = frame(tick + 1);
        if now.iter().zip(&next).any(|(a, b)| a != b) {
            ticks_with_motion += 1;
        }
        nudged += now
            .iter()
            .zip(&next)
            .filter(|(a, b)| braille(&a.text) && braille(&b.text) && a.text != b.text)
            .count();
    }
    assert!(
        ticks_with_motion >= 9,
        "only {ticks_with_motion} of 12 ticks moved"
    );
    assert!(nudged > 0, "no star moved within its cell");
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
    let streak = super::moving(96, 28, 24.0, 0).len() - super::flying(96, 28, 24.0, 0).len();
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
fn splash_stars_glide_between_whole_beats() {
    let at = |beat: f64| {
        super::flying(200, 60, beat, 0)
            .into_iter()
            .map(|spot| (spot.x, spot.y, spot.cell.text.to_string()))
            .collect::<Vec<_>>()
    };
    let moved_off_beat = (1..16)
        .filter(|step| step % 4 != 0)
        .any(|step| at(100.0 + f64::from(step) / 4.0) != at(100.0 + f64::from(step - 1) / 4.0));
    assert!(moved_off_beat, "stars only move on whole beats");
}

#[test]
fn a_star_at_the_wrap_edge_stays_on_screen() {
    assert_eq!(super::cell_of(240.0, 144.0, 120, 36), (119, 35));
    assert_eq!(super::cell_of(0.0, 0.0, 120, 36), (0, 0));
}

#[test]
fn the_galaxy_dust_paints_in_few_colours_and_long_runs() {
    let (cols, rows) = (223_u16, 67_u16);
    let cells = super::backdrop(cols, rows, 0);
    let shades: std::collections::BTreeSet<String> = cells
        .iter()
        .flat_map(|cell| [format!("{:?}", cell.fg), format!("{:?}", cell.bg)])
        .collect();
    assert!(shades.len() <= 40, "the haze uses {} colours", shades.len());
    assert!(
        shades.len() >= 6,
        "the haze is flat: {} colours",
        shades.len()
    );
    let changes: usize = cells
        .chunks(usize::from(cols))
        .map(|row| row.windows(2).filter(|pair| pair[0] != pair[1]).count())
        .sum();
    let per_row = changes / usize::from(rows);
    assert!(per_row <= 40, "{per_row} colour changes per row");
    let stars = super::space(cols, rows, 30, 0)
        .iter()
        .filter(|cell| braille(&cell.text))
        .count();
    assert!(stars >= 150, "only {stars} stars");
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
