use super::{Window, window};

#[test]
fn the_caret_maps_to_a_line_and_column() {
    let text = "one\ntwo\nthree";
    assert_eq!(
        window(text, 0, 0, 10),
        Window {
            start: 0,
            line: 0,
            col: 0
        }
    );
    assert_eq!(
        window(text, 5, 0, 10),
        Window {
            start: 0,
            line: 1,
            col: 1
        }
    );
    assert_eq!(
        window(text, 14, 0, 10),
        Window {
            start: 0,
            line: 2,
            col: 5
        }
    );
    assert_eq!(
        window(text, 99, 0, 10).line,
        2,
        "a caret past the end clamps"
    );
}

#[test]
fn the_window_follows_the_caret_and_otherwise_stays() {
    let text = "a\nb\nc\nd\ne\nf";
    assert_eq!(
        window(text, 10, 0, 3).start,
        3,
        "caret on line 5 with 3 rows shows 3..6"
    );
    assert_eq!(
        window(text, 0, 4, 3).start,
        0,
        "a caret above the window pulls it up"
    );
    assert_eq!(
        window(text, 4, 1, 3).start,
        1,
        "a visible caret leaves the scroll alone"
    );
}
