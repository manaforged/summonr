use super::{EditorKey, Hotkey, Hotkeys, available};

#[test]
fn ctrl_o_opens_settings_by_default_and_from_the_stored_chord_table() {
    let stored = [
        ("options", "ctrl+x o"),
        ("newline", "ctrl+j"),
        ("editor", "ctrl+g"),
        ("files", "ctrl+x e"),
        ("ledger", "ctrl+t"),
        ("agents", "ctrl+x a"),
        ("diff", "ctrl+x d"),
        ("copy", "ctrl+x y"),
        ("select", "ctrl+x t"),
        ("pane", "ctrl+x b"),
        ("search", "ctrl+x f"),
        ("history", "ctrl+r"),
        ("fold", "ctrl+x n"),
    ]
    .map(|(name, key)| (name.to_string(), key.to_string()))
    .into();
    for hotkeys in [Hotkeys::default(), Hotkeys::configured(&stored).unwrap()] {
        assert_eq!(hotkeys.action('o'), Some(Hotkey::Options));
        assert_eq!(hotkeys.label(Hotkey::Options), "ctrl+o");
        assert_eq!(hotkeys.chord('e'), Some(Hotkey::Files));
    }
    let mut chosen = stored;
    chosen.insert("fold".to_string(), "ctrl+x z".to_string());
    assert_eq!(
        Hotkeys::configured(&chosen).unwrap().chord('o'),
        Some(Hotkey::Options)
    );
}

#[test]
fn editor_line_keys_cannot_be_bound() {
    for key in ['u', 'k', 'w', 'a', 'b', 'e', 'f', 'l', 'n', 'p', 'x'] {
        assert!(!available(key));
        assert!(!Hotkeys::default().assign(Hotkey::Fold, key, false));
    }
}

#[test]
fn select_all_and_cut_reach_a_non_empty_composer_off_macos() {
    let off_mac = !cfg!(target_os = "macos");
    assert_eq!(EditorKey::yields('a', false), off_mac);
    assert_eq!(EditorKey::yields('x', false), off_mac);
    assert!(!EditorKey::yields('a', true));
    assert!(!EditorKey::yields('o', false));
}
