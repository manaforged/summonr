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
fn a_star_that_flashes_flashes_again_within_four_seconds() {
    let (cols, rows) = (96, 26);
    let window = u64::from(crate::prefs::FPS_DEFAULT) * 4;
    let mut lit = vec![false; usize::from(cols) * usize::from(rows)];
    let mut onsets = vec![0_u32; lit.len()];
    for tick in 0..window * 2 {
        let mut cells = vec![super::night(); lit.len()];
        super::stars(&mut cells, cols, rows, tick);
        for (at, cell) in cells.iter().enumerate() {
            let bright = cell.text == "✦";
            if bright && !lit[at] {
                onsets[at] += 1;
            }
            lit[at] = bright;
        }
    }
    let flashing: Vec<u32> = onsets.into_iter().filter(|count| *count > 0).collect();
    assert!(flashing.len() >= 8, "only {} stars flashed", flashing.len());
    assert!(flashing.iter().all(|count| *count >= 2), "{flashing:?}");
}
