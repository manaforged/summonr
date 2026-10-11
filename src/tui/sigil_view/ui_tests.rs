use super::*;
use crate::prefs::{Hotkey, Hotkeys};

#[test]
fn hints_render_one_separator_and_follow_rebound_keys() {
    let hints = Hints::new()
        .key(Key::UpDown, "move")
        .key(Key::Enter, "use")
        .key(Key::Hotkey(Hotkey::Agents), "children")
        .key(Key::Esc, "close");
    let mut keys = Hotkeys::default();
    assert_eq!(
        hints.text(&keys),
        "↑↓ move · ↵ use · ctrl+x a children · esc close"
    );
    assert!(keys.assign(Hotkey::Agents, 'g', false));
    assert!(hints.text(&keys).contains("ctrl+g children"));
}
