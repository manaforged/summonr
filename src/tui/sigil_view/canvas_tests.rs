use super::*;

#[test]
fn blank_cell_uses_the_shared_canvas_defaults() {
    let cell = Cell::blank();

    assert_eq!(cell.text, " ");
    assert_eq!(cell.fg, palette().text);
    assert_eq!(cell.bg, palette().base);
    assert!(!cell.bold);
    assert!(!cell.italic);
}
