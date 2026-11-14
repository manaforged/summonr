use super::nearest;
use crate::view::keys::bindings::Dir;

const LEFT: (f32, f32, f32, f32) = (0.0, 0.0, 600.0, 800.0);
const TOP_RIGHT: (f32, f32, f32, f32) = (606.0, 0.0, 600.0, 400.0);
const BOTTOM_RIGHT: (f32, f32, f32, f32) = (606.0, 406.0, 600.0, 394.0);

fn panes() -> impl Iterator<Item = (usize, (f32, f32, f32, f32))> {
    [(0, LEFT), (1, TOP_RIGHT), (2, BOTTOM_RIGHT)].into_iter()
}

#[test]
fn arrows_move_between_a_tall_pane_and_two_stacked_beside_it() {
    let from = |at: usize| {
        panes()
            .find(|(id, _)| *id == at)
            .map(|(_, b)| b)
            .expect("pane")
    };
    let others = |at: usize| panes().filter(move |(id, _)| *id != at);
    assert_eq!(nearest(from(1), Dir::Down, others(1)), Some(2));
    assert_eq!(nearest(from(2), Dir::Up, others(2)), Some(1));
    assert_eq!(nearest(from(2), Dir::Left, others(2)), Some(0));
    assert_eq!(nearest(from(0), Dir::Right, others(0)), Some(1));
    assert_eq!(nearest(from(0), Dir::Left, others(0)), None);
    assert_eq!(nearest(from(1), Dir::Up, others(1)), None);
}
