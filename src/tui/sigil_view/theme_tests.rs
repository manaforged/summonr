use super::*;

#[test]
fn hex_accepts_short_long_and_alpha_forms() {
    assert_eq!(
        hex("#abc").expect("short"),
        Rgba8 {
            r: 0xaa,
            g: 0xbb,
            b: 0xcc,
            a: 255
        }
    );
    assert_eq!(
        hex("102030").expect("plain"),
        Rgba8 {
            r: 0x10,
            g: 0x20,
            b: 0x30,
            a: 255
        }
    );
    assert_eq!(
        hex("#10203040").expect("alpha"),
        Rgba8 {
            r: 0x10,
            g: 0x20,
            b: 0x30,
            a: 0x40
        }
    );
    hex("#12345").unwrap_err();
    hex("not-a-color").unwrap_err();
}

#[test]
fn named_presets_select_the_accent() {
    assert_eq!(Palette::named("fantasy").accent, Palette::FANTASY.accent);
    assert_eq!(Palette::named("space").accent, Palette::SPACE.accent);
    assert_eq!(Palette::named("anything").base, Palette::SPACE.base);
}

#[test]
fn install_overlays_theme_files_on_the_preset() {
    let home = tempfile::tempdir().expect("home");
    let workspace = tempfile::tempdir().expect("workspace");
    std::fs::create_dir_all(workspace.path().join(".mage")).expect("dir");
    std::fs::write(
        workspace.path().join(".mage/theme.toml"),
        "accent = \"#ff0000\"\ntext = \"#00ff00\"\nunknown = \"#0000ff\"\n",
    )
    .expect("theme");

    install(
        "space",
        workspace.path(),
        Some(&home.path().join("config.toml")),
    );
    let active = palette();
    assert_eq!(
        active.accent,
        Rgba8 {
            r: 255,
            g: 0,
            b: 0,
            a: 255
        }
    );
    assert_eq!(
        active.text,
        Rgba8 {
            r: 0,
            g: 255,
            b: 0,
            a: 255
        }
    );
    assert_eq!(
        active.base,
        Palette::SPACE.base,
        "unset tokens keep the preset"
    );
    set(Palette::SPACE);
}

#[test]
fn the_example_theme_parses_and_resolves() {
    use std::collections::BTreeMap;

    let rows: BTreeMap<String, String> =
        toml::from_str(include_str!("../../../theme.example.toml")).expect("example theme");
    assert!(!rows.is_empty(), "example theme is empty");
    for (key, value) in &rows {
        hex(value).unwrap_or_else(|error| panic!("{key}: {error}"));
    }
}

#[test]
fn the_space_preset_matches_the_shipped_defaults() {
    let space = Palette::SPACE;
    assert_eq!(space.base, rgb(12, 12, 20));
    assert_eq!(space.text, rgb(200, 200, 205));
    assert_eq!(space.accent, rgb(105, 140, 210));
    assert_eq!(space.tool_bg, rgb(24, 26, 36));
    assert_eq!(space.diff_add_bg, rgb(24, 48, 32));
    assert_eq!(space.diff_del_bg, rgb(56, 28, 32));
    assert_eq!(Palette::FANTASY.accent, rgb(255, 208, 96));
}
