use super::*;
use crate::tui::sigil_view::canvas::ink;
use sigil::terminal::Ink;

#[test]
fn the_terminal_background_leaves_the_base_unpainted() {
    let dir = tempfile::tempdir().unwrap();
    let mut prefs = Prefs::default();
    drop(install(&prefs, dir.path(), None));
    assert!(matches!(ink(palette().base), Ink::Exact(_)));
    prefs.background = Background::Terminal;
    drop(install(&prefs, dir.path(), None));
    assert_eq!(ink(palette().base), Ink::Bare);
    assert_eq!(palette().text, Palette::SPACE.text);
}

#[test]
fn a_bad_theme_color_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("theme.toml"), "accent = \"nope\"\n").unwrap();
    let config = dir.path().join("config.toml");
    let problems = install(&Prefs::default(), dir.path(), Some(&config));
    let text = problems.join("\n");
    assert!(
        text.contains("theme color") && text.contains("nope"),
        "{text}"
    );
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

fn luminance(color: Rgba8) -> f64 {
    let channel = |value: u8| {
        let value = f64::from(value) / 255.0;
        if value <= 0.039_28 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(color.r) + 0.7152 * channel(color.g) + 0.0722 * channel(color.b)
}

fn contrast(one: Rgba8, two: Rgba8) -> f64 {
    let (one, two) = (luminance(one), luminance(two));
    (one.max(two) + 0.05) / (one.min(two) + 0.05)
}

#[test]
fn every_palette_keeps_text_hints_and_code_blocks_legible_on_base() {
    for (name, palette) in Palette::ALL {
        for (token, color, floor) in [
            ("text", palette.text, 4.5),
            ("gray", palette.gray, 4.5),
            ("dim", palette.dim, 3.0),
            ("code_comment", palette.code_comment, 3.0),
            ("accent", palette.accent, 3.0),
            ("code_bg", palette.code_bg, 1.35),
        ] {
            let ratio = contrast(color, palette.base);
            assert!(ratio >= floor, "{name} {token} is {ratio:.2}:1");
        }
        assert_ne!(palette.accent, palette.cyan, "{name} accent equals cyan");
    }
}
