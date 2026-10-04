use super::composer_hint;

#[test]
fn the_idle_composer_shows_no_hotkey_hint() {
    assert_eq!(composer_hint(None, 200, None), "");
}
