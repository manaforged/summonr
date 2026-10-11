use sigil::terminal::Ink;

#[test]
fn the_splash_field_covers_the_screen() {
    let cells = super::space(48, 16, 1);
    assert_eq!(cells.len(), 48 * 16);
    assert!(cells.iter().all(|cell| match cell.bg {
        Ink::Exact(colour) => colour.a == 255,
        _ => false,
    }));
}

#[test]
fn splash_has_at_most_two_bright_points_even_with_a_meteor_on_a_large_screen() {
    for tick in 0..880 {
        let cells = super::space(200, 60, tick);
        let bright = cells.iter().filter(|cell| cell.text == "✦").count();
        assert!(bright <= 2, "tick {tick}: {bright} simultaneous sparkles");
    }
}

#[test]
fn splash_stars_drift_one_cell_per_second_without_flashing() {
    let earlier = super::space(80, 24, 120);
    let later = super::space(80, 24, 145);
    assert!(earlier.iter().zip(&later).any(|(a, b)| a.text != b.text));
    for row in 0..24 {
        for col in 0..79 {
            assert_eq!(later[row * 80 + col].text, earlier[row * 80 + col + 1].text);
        }
    }
}

#[test]
fn splash_has_one_brief_sparkle_every_two_seconds_with_quiet_gaps() {
    let mut lit = false;
    let mut onsets = Vec::new();
    let mut lit_frames = 0;
    for tick in 120..420 {
        let cells = super::space(200, 60, tick);
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
