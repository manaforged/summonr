use super::*;
use crate::tui::sigil_view::canvas::ink;
use sigil::terminal::Ink;

#[test]
fn the_terminal_background_leaves_the_base_unpainted() {
    let dir = tempfile::tempdir().unwrap();
    let mut prefs = Prefs::default();
    install(&prefs, dir.path(), None);
    assert!(matches!(ink(palette().base), Ink::Exact(_)));
    prefs.background = Background::Terminal;
    install(&prefs, dir.path(), None);
    assert_eq!(ink(palette().base), Ink::Bare);
    assert_eq!(palette().text, Palette::SPACE.text);
}

#[test]
fn three_themes_remain_and_retired_names_still_load() {
    assert_eq!(Palette::named("light"), Palette::DAWN);
    assert_eq!(Palette::named("gruvbox"), Palette::SPACE);
    assert_eq!(Palette::named("nord"), Palette::NORD);
    assert_eq!(
        Palette::ALL.map(|(name, _)| name),
        ["space", "nord", "dawn"]
    );
}

#[test]
fn auto_follows_the_terminal_backdrop() {
    set_backdrop_light(true);
    assert_eq!(Palette::named("auto"), Palette::DAWN);
    set_backdrop_light(false);
    assert_eq!(Palette::named("auto"), Palette::SPACE);
}
