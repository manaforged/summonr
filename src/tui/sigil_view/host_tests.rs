use super::{restore, status};

#[test]
fn clean_quit_wins_channel_close_race() {
    assert_eq!(status(true, true), Some(0));
    assert_eq!(status(true, false), Some(1));
    assert_eq!(status(false, false), None);
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
