use super::input::{glass, overlay};
use super::logo::Composed;
use super::*;

fn composed() -> Composed {
    Composed {
        plate: glass([4, 2, 12, 5]),
        shimmer: None,
    }
}

#[test]
fn a_new_generation_keeps_the_rows_that_did_not_change() {
    let mut painted = Painted::default();
    let card = composed();
    painted.settle(1, &card);
    let before = painted.lines.clone();
    let mut changed = composed();
    overlay(&mut changed.plate, 4, 3, "x", palette().text);
    painted.settle(2, &changed);
    assert_eq!(painted.lines.len(), before.len());
    assert!(painted.lines[0] == before[0]);
    assert!(painted.lines[1] != before[1]);
    assert!(painted.lines[2] == before[2]);
}

#[test]
fn a_refresh_repaints_a_dirty_row_from_the_work_plate() {
    let mut painted = Painted::default();
    painted.settle(1, &composed());
    let untouched = painted.lines[2].clone();
    overlay(&mut painted.work, 5, 3, "x", palette().text);
    painted.refresh(vec![3, 3, 4]);
    assert_eq!(
        painted.lines[1].0.2.as_slice(),
        painted.work.cells_of(1).unwrap()
    );
    assert!(painted.lines[2] == untouched);
}
