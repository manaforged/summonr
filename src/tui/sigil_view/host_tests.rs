use super::restore;

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
