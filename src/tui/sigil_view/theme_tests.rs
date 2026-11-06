use super::*;
use crate::tui::sigil_view::canvas::ink;
use sigil::prelude::HostMode;
use sigil::terminal::Ink;

#[test]
fn the_terminal_background_leaves_the_base_unpainted() {
    let dir = tempfile::tempdir().unwrap();
    let mut prefs = Prefs {
        background: Background::Theme,
        ..Prefs::default()
    };
    assert_eq!(Background::default(), Background::Terminal);
    assert!(!Prefs::default().splash);
    drop(install(&prefs, dir.path(), None));
    assert!(matches!(ink(palette().base), Ink::Exact(_)));
    prefs.background = Background::Terminal;
    drop(install(&prefs, dir.path(), None));
    assert_eq!(ink(palette().base), Ink::Bare);
    assert_eq!(palette().text, Palette::SPACE.text);
}

#[test]
fn a_bad_theme_color_is_reported() {
    for value in ["nope", "éa", "😀ab", "#gg1122"] {
        assert!(hex(value).is_err(), "invalid color accepted: {value}");
    }
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
fn named_themes_remain_and_retired_names_still_load() {
    assert_eq!(Palette::named("light"), Palette::DAWN);
    assert_eq!(Palette::named("gruvbox"), Palette::SPACE);
    assert_eq!(Palette::named("nord"), Palette::NORD);
    assert_eq!(
        Palette::ALL.map(|(name, _)| name),
        ["space", "nord", "dawn", "paper", "iron"]
    );
}

#[test]
fn auto_follows_the_terminal_backdrop() {
    set_backdrop_light(true);
    assert_eq!(Palette::named("auto"), Palette::DAWN);
    set_backdrop_light(false);
    assert_eq!(Palette::named("auto"), Palette::SPACE);
}

#[test]
fn applying_custom_colors_repaints_a_mounted_view_without_changing_the_theme_name() {
    use crate::tui::sigil_view::{app::App, profile::info, view::root};
    use sigil::Host;

    use sigil::terminal::Screen;
    use tokio::sync::mpsc::unbounded_channel;

    sigil::reactive::Owner::scope("installed-palette", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let folder = workspace.path().join(".summonr");
        std::fs::create_dir(&folder).expect("theme folder");
        let file = folder.join("theme.toml");
        std::fs::write(&file, "base = \"#102030\"\n").expect("initial theme");
        let (commands, _receiver) = unbounded_channel();
        let mut context = info();
        context.config = Some(workspace.path().join("config.toml"));
        context.prefs.background = Background::Theme;
        let app = App::new(context, workspace.path().to_owned(), commands);
        app.resize(80, 24);
        app.refresh();
        let mut host = Host::new(HostMode::Cell);
        let tree = app.clone();
        host.mount(move || root(tree));
        let mut screen = Screen::new(80, 24);
        let mut background = || {
            screen.frame(&mut host, [], (80, 24)).expect("settle theme");
            screen
                .frame(&mut host, [], (80, 24))
                .expect("paint theme")
                .get(0, 0)
                .expect("background cell")
                .bg
        };
        assert_eq!(
            background(),
            Ink::Exact(Rgba8 {
                r: 16,
                g: 32,
                b: 48,
                a: 255
            })
        );
        std::fs::write(&file, "base = \"#405060\"\n").expect("updated theme");
        app.apply_theme();
        app.refresh();
        assert_eq!(
            background(),
            Ink::Exact(Rgba8 {
                r: 64,
                g: 80,
                b: 96,
                a: 255
            })
        );
        app.toggle_background();
        assert_eq!(background(), Ink::Bare);
    });
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
    for (name, palette) in Palette::ALL
        .into_iter()
        .chain([("auto", Palette::SPACE), ("auto", Palette::DAWN)])
    {
        set_backdrop_light(palette == Palette::DAWN);
        let theme = style(palette);
        use sigil::prelude::Token;
        for token in [Token::Surface, Token::Overlay] {
            assert_eq!(theme.color(token), palette.base, "{name}: {token:?}");
        }
        assert_eq!(theme.color(Token::OnSurface), palette.text, "{name}");
        assert_eq!(theme.color(Token::OnSurfaceMuted), palette.dim, "{name}");
        assert_eq!(theme.color(Token::OverlayBorder), palette.dim, "{name}");
        assert_eq!(theme.color(Token::SurfaceSubtle), palette.tool_bg, "{name}");
        assert_eq!(theme.color(Token::Gray10), palette.gray, "{name}");
        sigil::reactive::Owner::scope("themed-modal", || {
            use crate::tui::sigil_view::{app::App, profile::info, view::root};
            let workspace = tempfile::tempdir().expect("workspace");
            let (commands, _receiver) = tokio::sync::mpsc::unbounded_channel();
            let mut context = info();
            context.prefs.theme = name.to_string();
            context.prefs.background = Background::Theme;
            let app = App::new(context, workspace.path().to_owned(), commands);
            app.resize(110, 40);
            let mut host = sigil::Host::new(HostMode::Cell);
            let tree = app.clone();
            host.mount(move || root(tree));
            let mut screen = sigil::terminal::Screen::new(110, 40);
            let scenes: [(fn(&App), &str); 3] = [
                (App::toggle_options, "Settings"),
                (
                    |app| {
                        app.toggle_options();
                        app.toggle_agents();
                    },
                    "Status",
                ),
                (
                    |app| {
                        app.close_status_sheet();
                        app.open_theme();
                    },
                    "Theme editor",
                ),
            ];
            for (open, title) in scenes {
                open(&app);
                for splash in [true, false] {
                    app.splash.set(splash);
                    let expected = if splash && [Palette::DAWN, Palette::PAPER].contains(&palette) {
                        Palette::SPACE
                    } else {
                        palette
                    };
                    screen.frame(&mut host, [], (110, 40)).expect("settle");
                    let grid = screen.frame(&mut host, [], (110, 40)).expect("modal frame");
                    let text = grid.text();
                    let (row, line) = text
                        .lines()
                        .enumerate()
                        .find(|(_, line)| line.contains(title))
                        .expect("modal title");
                    let at = line.find(title).expect("title position");
                    let col = sigil::app::cell_width(&line[..at]);
                    let cell = grid.get(col, row).expect("title cell");
                    assert_eq!(
                        cell.bg,
                        Ink::Exact(expected.base),
                        "{name}: {title} background, splash={splash}"
                    );
                    assert_eq!(cell.fg, Ink::Exact(expected.text), "{name}: {title} text");
                }
            }
        });
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
