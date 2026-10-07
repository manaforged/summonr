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
