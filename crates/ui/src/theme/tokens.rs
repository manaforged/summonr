use sigil::prelude::{Color, Theme};
use sigil::style::ThemeValue;

use super::Palette;
use crate::canvas::blend;

pub(crate) fn style(palette: Palette) -> Theme {
    let mut theme = Theme::dark();
    theme.gray.steps = [
        palette.black,
        palette.base,
        palette.tool_bg,
        palette.code_bg,
        palette.user_bg,
        palette.code_bg,
        palette.dim,
        palette.gray,
        palette.dim,
        palette.gray,
        palette.dim,
        palette.text,
    ];
    let on_solid = Color {
        a: 255,
        ..palette.base
    };
    for (scale, color) in [
        (&mut theme.accent_scale, palette.accent),
        (&mut theme.success, palette.green),
        (&mut theme.warn, palette.yellow),
        (&mut theme.error, palette.error),
        (&mut theme.info, palette.cyan),
    ] {
        scale.steps = steps(palette.base, color, palette.text);
        scale.on_solid = on_solid;
    }
    for (name, color) in [
        ("surface", palette.base),
        ("surface-hover", palette.tool_bg),
        ("surface-pressed", palette.user_bg),
        ("surface-pressed-strong", palette.dim),
        ("surface-subtle", palette.tool_bg),
        ("surface-inset", palette.black),
        ("on-surface", palette.text),
        ("on-surface-muted", palette.dim),
        (
            "overlay",
            Color {
                a: 255,
                ..palette.base
            },
        ),
        ("overlay-border", palette.dim),
        ("border", palette.dim),
        ("border-strong", palette.gray),
        ("border-subtle", blend(palette.base, palette.dim, 0.5)),
        ("table-header", palette.tool_bg),
        (
            "scrim",
            Color {
                a: 160,
                ..palette.black
            },
        ),
        ("field", palette.tool_bg),
        ("accent", palette.accent),
        ("accent-hover", blend(palette.base, palette.accent, 0.8)),
        ("on-accent", theme.accent_scale.on_solid),
        ("on-solid", theme.accent_scale.on_solid),
        ("ring", palette.accent),
    ] {
        theme.register_token(name, ThemeValue::Color(color));
    }
    if palette.light() {
        theme.accent_scale.steps = steps(palette.base, palette.accent, palette.text);
    }
    theme
}

pub(crate) fn steps(base: Color, color: Color, text: Color) -> [Color; 12] {
    [
        base,
        blend(base, color, 0.04),
        blend(base, color, 0.08),
        blend(base, color, 0.12),
        blend(base, color, 0.16),
        blend(base, color, 0.22),
        blend(base, color, 0.32),
        blend(base, color, 0.5),
        color,
        blend(base, color, 0.9),
        color,
        blend(color, text, 0.3),
    ]
}
