use super::window;

#[test]
fn the_window_keeps_summonrs_desktop_contract() {
    let config = window();
    assert_eq!(config.title, "Summonr");
    assert_eq!(config.size, (1280, 800));
    assert!(!config.esc_exits);
}
