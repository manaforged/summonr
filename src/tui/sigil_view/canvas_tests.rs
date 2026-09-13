use super::*;

#[test]
fn blank_cell_uses_the_shared_canvas_defaults() {
    let cell = Cell::blank();

    assert_eq!(cell.text, " ");
    assert_eq!(cell.fg, TEXT);
    assert_eq!(cell.bg, BASE);
    assert!(!cell.bold);
    assert!(!cell.italic);
}
