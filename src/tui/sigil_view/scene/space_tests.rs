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
    assert!(quiet.iter().all(|cell| !braille(&cell.text)));
    let flying = super::space(96, 28, 24, super::QUIET);
    let streak = flying.iter().filter(|cell| braille(&cell.text)).count();
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
