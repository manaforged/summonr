use super::{Axis, Part, Shape};

const fn pane(at: usize, size: u32) -> Part {
    Part {
        shape: Shape::Pane(at),
        size,
    }
}

#[test]
fn splitting_right_then_down_stacks_two_panes_beside_the_first() {
    let mut shape = Shape::default();
    assert!(shape.split(0, Axis::Right, 1, 1200.0));
    assert!(shape.split(1, Axis::Down, 2, 800.0));
    assert_eq!(
        shape,
        Shape::Split {
            axis: Axis::Right,
            parts: vec![
                pane(0, 600),
                Part {
                    shape: Shape::Split {
                        axis: Axis::Down,
                        parts: vec![pane(1, 400), pane(2, 400)],
                    },
                    size: 600,
                },
            ],
        }
    );
    assert_eq!(shape.panes(), vec![0, 1, 2]);
}

#[test]
fn splitting_along_the_same_axis_adds_a_column_instead_of_nesting() {
    let mut shape = Shape::row(2);
    assert!(shape.split(1, Axis::Right, 2, 900.0));
    let Shape::Split { axis, parts } = &shape else {
        panic!("expected a split, got {shape:?}");
    };
    assert_eq!(*axis, Axis::Right);
    assert_eq!(parts.len(), 3);
}

#[test]
fn closing_a_stacked_pane_gives_its_space_back_and_renumbers() {
    let mut shape = Shape::default();
    shape.split(0, Axis::Right, 1, 1200.0);
    shape.split(1, Axis::Down, 2, 800.0);
    shape.remove(1);
    assert_eq!(
        shape,
        Shape::Split {
            axis: Axis::Right,
            parts: vec![pane(0, 600), pane(1, 600)],
        }
    );
    shape.remove(0);
    assert_eq!(shape, Shape::Pane(0));
}

#[test]
fn dragged_sizes_land_on_the_nested_split() {
    let mut shape = Shape::default();
    shape.split(0, Axis::Right, 1, 1200.0);
    shape.split(1, Axis::Down, 2, 800.0);
    shape.set_sizes(&[1], &[250.0, 550.0]);
    shape.swap(0, 2);
    let Shape::Split { parts, .. } = &shape else {
        panic!("expected a split, got {shape:?}");
    };
    assert_eq!(parts[0].shape, Shape::Pane(2));
    assert_eq!(
        parts[1].shape,
        Shape::Split {
            axis: Axis::Down,
            parts: vec![pane(1, 250), pane(0, 550)],
        }
    );
}
