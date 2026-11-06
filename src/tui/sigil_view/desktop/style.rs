use sigil::prelude::{Elevation, LayoutStyle, NodeBuilder, Paint, Radius, TextBuilder, TextContent, Token, Widget, container};

use crate::tui::hub::Status;

/// Spacing step: 4 points.
pub(super) const XS: f32 = 4.0;
/// Spacing step: 8 points.
pub(super) const S: f32 = 8.0;
/// Spacing step: 12 points.
pub(super) const M: f32 = 12.0;
/// Spacing step: 16 points.
pub(super) const L: f32 = 16.0;
/// Spacing step: 24 points.
pub(super) const XL: f32 = 24.0;

/// Inset of every mark from the reading column's edge: text, icons, bubbles, send.
pub(super) const GUTTER: f32 = L;
/// The track a scroll view keeps at its right edge.
pub(super) const SCROLLBAR: f32 = S;
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
/// The most agents shown side by side.
pub(super) const PANES: usize = 4;
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
pub(super) const NEEDS_DOT: Token = Token::Warn9;
/// The status dot of a finished agent the user has not seen.
pub(super) const UNREAD_DOT: Token = Token::Info9;
/// The edge of the focused pane.
pub(super) const FOCUS_RING: Token = Token::Accent9;

/// An agent's status mark in an `ICON` slot, so it lines up with icons: spinner, a dot, or empty.
pub(super) fn mark(status: Status) -> NodeBuilder {
    let slot = container().layout(
        LayoutStyle::flex_row()
            .width(ICON)
            .height(ICON)
            .shrink(0.0)
            .align_items(sigil::prelude::AlignItems::Center)
            .justify_content(sigil::prelude::JustifyContent::Center),
    );
    let tone = match status {
        Status::Working => {
            return slot.child(sigil::widgets::spinner().layout(LayoutStyle::block().width(MARK).height(MARK)));
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
        .layout(LayoutStyle::block().width(DOT).height(DOT).shrink(0.0))
        .rounded(Radius::Full)
        .bg(tone)
}

/// The text roles of the desktop window; every string uses one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Text {
    /// A page title: 22 semibold.
    Display,
    /// A region title such as the chat name: 15 semibold.
    Title,
    /// Reading text: 15 regular.
    Body,
    /// Interface text in rows and controls: 13 regular.
    Label,
    /// Secondary interface text: 13 regular, muted.
    Hint,
    /// A small section heading: 12 semibold, muted.
    Heading,
    /// Small secondary detail such as counts and shortcuts: 12 regular, muted.
    Meta,
}

impl Text {
    const fn spec(self) -> (f32, u16, Token) {
        match self {
            Self::Display => (24.0, 600, Token::OnSurface),
            Self::Title => (BODY, 600, Token::OnSurface),
            Self::Body => (BODY, 400, Token::OnSurface),
            Self::Label => (14.0, 400, Token::OnSurface),
            Self::Hint => (14.0, 400, SECONDARY),
            Self::Heading => (13.0, 600, SECONDARY),
            Self::Meta => (13.0, 400, SECONDARY),
        }
    }

}

/// A text node styled as `role`.
pub(super) fn text(role: Text, content: impl Into<TextContent>) -> TextBuilder {
    let (size, weight, color) = role.spec();
    sigil::prelude::text(content)
        .font_size(size)
        .font_weight(weight)
        .text_color(color)
}
