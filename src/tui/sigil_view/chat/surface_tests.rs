use super::{pin_extent, visible_range};

#[test]
fn pinned_scroll_at_content_height_still_shows_the_last_lines() {
    let (start, end) = visible_range(1000.0, 10.0, 100, 50.0);
    assert_eq!((start, end), (93, 100));
}

#[test]
fn a_zero_height_pane_does_not_pin_past_the_last_window() {
    let (start, end) = visible_range(1000.0, 10.0, 100, 0.0);
    assert!(start < 100);
    assert_eq!(end, 100);
}

#[test]
fn pin_extent_uses_a_cell_when_the_viewport_collapsed() {
    assert_eq!(pin_extent(1000.0, 0.0, 10.0), 990.0);
}

#[test]
fn an_empty_transcript_has_no_window() {
    assert_eq!(visible_range(40.0, 10.0, 0, 50.0), (0, 0));
}
