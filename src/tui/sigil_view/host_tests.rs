use super::{mouse, restore, status};

#[test]
fn clean_quit_wins_channel_close_race() {
    assert_eq!(status(true, true), Some(0));
    assert_eq!(status(true, false), Some(1));
    assert_eq!(status(false, false), None);
}

#[test]
fn selection_returns_mouse_control_to_the_terminal() {
    let mut bytes = Vec::new();
    mouse(&mut bytes, true).unwrap_or_else(|error| panic!("disable mouse: {error}"));
    let disabled = String::from_utf8_lossy(&bytes);
    assert!(disabled.contains("\x1b[?1000l"), "{disabled:?}");

    bytes.clear();
    mouse(&mut bytes, false).unwrap_or_else(|error| panic!("enable mouse: {error}"));
    let enabled = String::from_utf8_lossy(&bytes);
    assert!(enabled.contains("\x1b[?1000h"), "{enabled:?}");
}

#[test]
fn restoration_ends_synchronized_output_and_terminal_modes() {
    let mut bytes = Vec::new();
    restore(&mut bytes).unwrap_or_else(|error| panic!("{error}"));
    let output = String::from_utf8_lossy(&bytes);

    assert!(output.contains("\x1b[?2026l"), "{output:?}");
    assert!(output.contains("\x1b[?2004l"), "{output:?}");
    assert!(output.contains("\x1b[?25h"), "{output:?}");
    assert!(output.contains("\x1b[?1049l"), "{output:?}");
}
