use super::{Axis, Part, Shape, WHOLE};

const fn pane(at: usize, share: u32) -> Part {
    Part {
        shape: Shape::Pane(at),
        share,
    }
}

fn split(axis: Axis, parts: Vec<Part>) -> Shape {
    Shape::Split { axis, parts }
}

#[test]
fn splitting_right_then_down_stacks_two_panes_beside_the_first() {
    let mut shape = Shape::default();
    assert!(shape.split(0, Axis::Right, 1));
    assert!(shape.split(1, Axis::Down, 2));
    let half = WHOLE / 2;
    let stack = split(Axis::Down, vec![pane(1, half), pane(2, half)]);
    assert_eq!(
        shape,
        split(
            Axis::Right,
            vec![
                pane(0, half),
                Part {
                    shape: stack,
                    share: half
                }
            ]
        )
    );
    assert_eq!(shape.panes(), vec![0, 1, 2]);
}

#[test]
fn splitting_along_the_same_axis_halves_that_pane_into_a_new_column() {
    let mut shape = Shape::row(2);
    assert!(shape.split(1, Axis::Right, 2));
    assert_eq!(
        shape,
        split(
            Axis::Right,
            vec![pane(0, WHOLE), pane(1, WHOLE / 2), pane(2, WHOLE / 2)]
        )
    );
}

#[test]
fn closing_a_stacked_pane_gives_its_share_back_and_renumbers() {
    let mut shape = Shape::default();
    shape.split(0, Axis::Right, 1);
    shape.split(1, Axis::Down, 2);
    shape.remove(1);
    assert_eq!(
        shape,
        split(Axis::Right, vec![pane(0, WHOLE / 2), pane(1, WHOLE / 2)])
    );
    shape.remove(0);
    assert_eq!(shape, Shape::Pane(0));
}

#[test]
fn dragged_pixel_sizes_become_shares_on_the_nested_split() {
    let mut shape = Shape::default();
    shape.split(0, Axis::Right, 1);
    shape.split(1, Axis::Down, 2);
    shape.set_sizes(&[1], &[200.0, 600.0]);
    shape.swap(0, 2);
    let Shape::Split { parts, .. } = &shape else {
        panic!("expected a split, got {shape:?}");
    };
    assert_eq!(parts[0].shape, Shape::Pane(2));
    assert_eq!(
        parts[1].shape,
        split(Axis::Down, vec![pane(1, WHOLE / 4), pane(0, WHOLE * 3 / 4)])
    );
}

#[test]
fn a_layout_saved_with_pixel_sizes_loads_them_as_shares() {
    let saved = r#"{"split":{"axis":"right","parts":[{"shape":{"pane":0},"size":300},{"shape":{"pane":1},"size":900}]}}"#;
    let shape: Shape = serde_json::from_str(saved).expect("saved shape");
    assert_eq!(shape, split(Axis::Right, vec![pane(0, 300), pane(1, 900)]));
}
