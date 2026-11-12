use sigil::prelude::{
    Color, Elevation, NodeBuilder, Paint, Radius, Space, TextBuilder, TextContent, Token, Widget,
    container,
};

use summonr_agent::hub::Status;
use summonr_agent::prefs::Tint;

use crate::canvas::rgb;

/// Spacing step: 4 points.
pub(super) const XS: Space = Space::Xs;
/// Spacing step: 8 points.
pub(super) const S: Space = Space::Sm;
/// Spacing step: 12 points.
pub(super) const M: Space = Space::Md;
/// Spacing step: 16 points.
pub(super) const L: Space = Space::Lg;
/// Spacing step: 24 points.
pub(super) const XL: Space = Space::Xl;

/// Inset of every mark from the reading column's edge: text, icons, bubbles, send.
pub(super) const GUTTER: f32 = 16.0;
/// The track a scroll view keeps at its right edge.
pub(super) const SCROLLBAR: Space = S;
/// The space the editor keeps before its first glyph.
pub(super) const CARET: f32 = 1.0;

/// Line height of body text.
pub(super) const LINE: f32 = 22.0;
/// Body text size, shared by messages and the composer.
pub(super) const BODY: f32 = 15.0;

/// How much larger than Sigil's defaults the named text sizes draw: 14 becomes 15.
pub(super) const TEXT_SCALE: f32 = 15.0 / 14.0;

/// Icon size everywhere.
pub(super) const ICON: f32 = 16.0;
/// A square control: icon buttons, send, and the slot rows align to.
pub(super) const CONTROL: f32 = 28.0;
/// The status mark slot and spinner size.
pub(super) const MARK: f32 = 14.0;
/// A status dot.
pub(super) const DOT: f32 = 8.0;
/// Every anchored menu and popover.
pub(super) const MENU: f32 = 288.0;
/// Every modal, including the palette.
pub(super) const DIALOG: f32 = 520.0;
/// The narrowest a pane may get, so a transcript keeps a readable line.
pub(super) const PANE_WIDTH: f32 = 360.0;
/// The shortest a pane may get.
pub(super) const PANE_HEIGHT: f32 = 240.0;
/// The height of a chart's plot.
pub(super) const CHART: f32 = 120.0;
/// The width of the focused pane's edge.
pub(super) const RING: f32 = 2.0;

/// The lift of controls that float on the canvas, such as the composer.
pub(super) const FLOATING: Elevation = Elevation::Lg;
/// The lift of cards and menus that sit above everything else.
pub(super) const OVERLAY: Elevation = Elevation::Xl;

/// The window canvas behind the chat.
pub(super) const CANVAS: Token = Token::Surface;
/// The sidebar and other recessed panels.
pub(super) const RECESSED: Token = Token::SurfaceSubtle;
/// Cards and fields that sit on the canvas.
pub(super) const RAISED: Token = Token::Surface;
/// A tinted fill for the user's own messages and code.
pub(super) const TINT: Token = Token::SurfaceHover;
/// Hairlines between regions.
pub(super) const DIVIDER: Token = Token::BorderSubtle;
/// The outline of cards and fields.
pub(super) const OUTLINE: Token = Token::Border;
/// Secondary text: hints, paths, meta.
pub(super) const SECONDARY: Token = Token::OnSurfaceMuted;
/// Text that reports a failure.
pub(super) const DANGER: Token = Token::Error11;
/// Text that reports a success.
pub(super) const POSITIVE: Token = Token::Success11;
/// The status dot of work in progress.
pub(super) const ACTIVE_DOT: Token = Token::Accent9;
/// The status dot of finished work.
pub(super) const DONE_DOT: Token = Token::Success9;
/// The status dot of failed or stopped work.
pub(super) const FAILED_DOT: Token = Token::Error9;
/// The status dot of an agent that needs the user.
pub(super) const NEEDS_DOT: Token = Token::Warning9;
/// The status dot of a finished agent the user has not seen.
pub(super) const UNREAD_DOT: Token = Token::Info9;
/// The bars of a chart.
pub(super) const CHART_BAR: Token = Token::Accent9;
/// The edge of the focused pane.
pub(super) const FOCUS_RING: Token = Token::Accent9;
/// Will's size where he floats over the main column.
pub(super) const WILL: f32 = 48.0;

/// Will's paints for one tint on one theme.
#[derive(Clone, Copy)]
pub(super) struct Ink {
    pub(super) edge: Color,
    pub(super) mid: Color,
    pub(super) core: Color,
    pub(super) eye: Color,
    pub(super) glow: Color,
    pub(super) warn: Color,
    pub(super) bad: Color,
}

/// Will's paints for `tint` on a light or dark theme.
pub(super) const fn will_ink(tint: Tint, light: bool) -> Ink {
    let [edge, mid, core, eye, glow] = match (tint, light) {
        (Tint::Auto, true) => return will_ink(Tint::Flame, true),
        (Tint::Auto, false) => return will_ink(Tint::Wisp, false),
        (Tint::Wisp, false) => [0x4f86e8, 0xaac8ff, 0xf4f6ff, 0x05050a, 0x6aa0ff],
        (Tint::Wisp, true) => [0x4084d6, 0x93b9ee, 0xf6f9ff, 0x1c1d1a, 0x4084d6],
        (Tint::Flame, false) => [0xf0652b, 0xf6a54a, 0xfff3d6, 0x1a0f08, 0xff8a4a],
        (Tint::Flame, true) => [0xd9541f, 0xf0a048, 0xfff4dc, 0x1c1d1a, 0xd9541f],
        (Tint::Foxfire, false) => [0x2fbf71, 0x9ee8bf, 0xf0fff4, 0x04100a, 0x3ad6b8],
        (Tint::Foxfire, true) => [0x16934f, 0x86d6a8, 0xf2fff6, 0x1c1d1a, 0x16934f],
        (Tint::Arcane, false) => [0x8b5cf6, 0xd4b4ff, 0xfbf4ff, 0x0a0514, 0xa070ff],
        (Tint::Arcane, true) => [0x6d3fd6, 0xbf9bf2, 0xfaf5ff, 0x1c1d1a, 0x6d3fd6],
    };
    let [warn, bad] = if light {
        [0xa87a12, 0xb03a2e]
    } else {
        [0xdcc85a, 0xdc5a5a]
    };
    Ink {
        edge: hex(edge),
        mid: hex(mid),
        core: hex(core),
        eye: hex(eye),
        glow: hex(glow),
        warn: hex(warn),
        bad: hex(bad),
    }
}

const fn hex(value: u32) -> Color {
    let [_, r, g, b] = value.to_be_bytes();
    rgb(r, g, b)
}

/// An agent's status mark in an `ICON` slot, so it lines up with icons: spinner, a dot, or empty.
pub(super) fn mark(status: Status) -> NodeBuilder {
    let slot = sigil::prelude::row()
        .gap(sigil::prelude::Space::None)
        .width(ICON)
        .height(ICON)
        .shrink(0.0)
        .align_items(sigil::prelude::AlignItems::Center)
        .justify_content(sigil::prelude::JustifyContent::Center);
    let tone = match status {
        Status::Working => {
            return slot.child(sigil::widgets::spinner().width(MARK).height(MARK));
        }
        Status::NeedsYou => NEEDS_DOT,
        Status::Review => UNREAD_DOT,
        Status::Failed => FAILED_DOT,
        Status::Idle => return slot,
    };
    slot.child(dot(tone))
}

/// A `DOT` circle in `tone`.
pub(super) fn dot(tone: Token) -> NodeBuilder {
    container()
        .width(DOT)
        .height(DOT)
        .shrink(0.0)
        .radius(Radius::Full)
        .background(tone)
}

/// The text roles of the desktop window; every string uses one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Text {
    /// A region title such as the chat name: 15 semibold.
    Title,
    /// Reading text: 15 regular.
    Body,
    /// Interface text in rows and controls: 14 regular.
    Label,
    /// A small section heading: 13 semibold, muted.
    Heading,
    /// Small secondary detail such as counts and shortcuts: 13 regular, muted.
    Meta,
}

impl Text {
    const fn spec(self) -> (f32, u16, Token) {
        match self {
            Self::Title => (BODY, 600, Token::OnSurface),
            Self::Body => (BODY, 400, Token::OnSurface),
            Self::Label => (14.0, 400, Token::OnSurface),
            Self::Heading => (13.0, 600, SECONDARY),
            Self::Meta => (13.0, 400, SECONDARY),
        }
    }
}

/// A text node styled as `role`.
pub(super) fn text(role: Text, content: impl Into<TextContent>) -> TextBuilder {
    let (size, weight, color) = role.spec();
    sigil::prelude::text(content)
        .font_size(size, "the desktop's text role")
        .font_weight(weight)
        .text_color(color)
}

/// The GUI view's theme: the backdrop color with small lifts toward the text color.
pub(crate) fn gui(palette: crate::theme::Palette) -> sigil::prelude::Theme {
    let mut theme = crate::theme::style(palette);
    let backdrop = if palette.base.a == 0 {
        sigil::terminal::caps()
            .and_then(|caps| caps.background())
            .unwrap_or(sigil::prelude::Color {
                a: 255,
                ..palette.base
            })
    } else {
        palette.base
    };
    let backdrop = sigil::prelude::Color { a: 255, ..backdrop };
    let lift = |amount: f32| crate::canvas::blend(backdrop, palette.text, amount);
    theme.gray.steps = [
        backdrop,
        lift(0.025),
        lift(0.05),
        lift(0.08),
        lift(0.11),
        lift(0.15),
        lift(0.2),
        lift(0.3),
        lift(0.45),
        lift(0.52),
        lift(0.62),
        palette.text,
    ];
    for (scale, color) in [
        (&mut theme.accent_scale, palette.accent),
        (&mut theme.success, palette.green),
        (&mut theme.warn, palette.yellow),
        (&mut theme.error, palette.error),
        (&mut theme.info, palette.cyan),
    ] {
        scale.steps = crate::theme::tokens::steps(backdrop, color, palette.text);
        scale.on_solid = backdrop;
    }
    for (name, color) in [
        ("surface", backdrop),
        ("surface-hover", lift(0.06)),
        ("surface-pressed", lift(0.1)),
        ("surface-pressed-strong", lift(0.16)),
        ("surface-subtle", lift(0.03)),
        ("surface-inset", lift(0.03)),
        ("field", lift(0.03)),
        ("table-header", lift(0.03)),
        ("overlay", backdrop),
        ("overlay-border", lift(0.15)),
        ("border", lift(0.12)),
        ("border-subtle", lift(0.07)),
        ("border-strong", lift(0.22)),
        ("on-surface-muted", lift(0.7)),
        ("placeholder", lift(0.52)),
        (
            "accent-hover",
            crate::canvas::blend(backdrop, palette.accent, 0.85),
        ),
        ("on-accent", backdrop),
        ("on-solid", backdrop),
    ] {
        theme = theme.token(name, sigil::style::ThemeValue::Color(color));
    }
    theme
}
