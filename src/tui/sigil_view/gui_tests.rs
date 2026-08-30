use super::window;

#[test]
fn the_window_keeps_summoners_desktop_contract() {
    let config = window();
    assert_eq!(config.title, "Summoner");
    assert_eq!(config.size, (1280, 800));
    assert!(!config.esc_exits);
}
