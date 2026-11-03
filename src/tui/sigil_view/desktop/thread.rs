use sigil::prelude::Paint;
use sigil::prelude::Widget;
use sigil::prelude::{
    AlignItems, ControlSize, JustifyContent, LayoutStyle, LogicalKey, ModifiersState, NodeBuilder,
    Shortcut, Token, Tone, button, container, icon_button, node, notice_band, spacer,
};

use super::{App, chat};
use crate::session;

mod composer;
mod mentions;
pub(in crate::tui::sigil_view) mod status;
mod title;

pub(super) const XS: f32 = 4.0;
pub(super) const S: f32 = 8.0;
pub(super) const M: f32 = 12.0;
pub(super) const L: f32 = 16.0;
pub(super) const XL: f32 = 24.0;
pub(super) const BODY: f32 = 14.0;
pub(super) const META: f32 = 13.0;
const COLUMN: f32 = 760.0;
const EXPLAIN: &str = "Explain this repo";
const ADD_PROVIDER: &str = "Add a provider";

pub(super) fn quiet(
    content: impl Into<sigil::prelude::TextContent>,
) -> sigil::prelude::TextBuilder {
    sigil::prelude::text(content)
        .nowrap()
        .font_size(META)
        .text_color(Token::OnSurfaceMuted)
}

fn column(child: NodeBuilder) -> NodeBuilder {
    reading(child, false)
}

fn reading(child: NodeBuilder, grow: bool) -> NodeBuilder {
    let outer = LayoutStyle::flex_row()
        .fill_width()
        .padding_x(XL)
        .justify_content(JustifyContent::Center);
    let inner = LayoutStyle::flex_column()
        .fill_width()
        .max_width(COLUMN)
        .align_items(AlignItems::Stretch);
    let (outer, inner) = if grow {
        (
            outer.grow(1.0).fill_height().min_size(0.0, 0.0),
            inner.fill_height().min_size(0.0, 0.0),
        )
    } else {
        (outer.shrink(0.0), inner)
    };
    container()
        .layout(outer)
        .child(container().layout(inner).child(child))
}

pub(super) fn main_col(app: App) -> NodeBuilder {
    let input = app.clone();
    let transcript_app = app.clone();
    let blank = {
        let model = app.model;
        sigil::reactive::derived(move || model.get().entries.is_empty())
    };
    container()
        .layout(
            LayoutStyle::flex_column()
                .grow(1.0)
                .fill_width()
                .fill_height()
                .min_size(0.0, 0.0)
                .padding_bottom(M)
                .align_items(AlignItems::Stretch),
        )
        .bg(Token::Surface)
        .child(
            container()
                .layout(
                    LayoutStyle::flex_column()
                        .fill_width()
                        .grow(1.0)
                        .min_size(0.0, 0.0),
                )
                .children_reactive(move || {
                    if blank.get() {
                        return Vec::new();
                    }
                    let app = transcript_app.clone();
                    vec![sigil::reactive::untrack(|| {
                        reading(
                            container()
                                .layout(
                                    LayoutStyle::flex_column()
                                        .fill_width()
                                        .fill_height()
                                        .grow(1.0)
                                        .min_size(0.0, 0.0)
                                        .padding_top(L),
                                )
                                .child(node(move || {
                                    chat::transcript(app.clone(), (L, L), empty_home)
                                })),
                            true,
                        )
                    })]
                }),
        )
        .child(
            container()
                .layout(LayoutStyle::block().fill_width().shrink(0.0))
                .children_reactive({
                    let home = app.clone();
                    move || {
                        if blank.get() {
                            vec![column(starters(&home))]
                        } else {
                            Vec::new()
                        }
                    }
                }),
        )
        .child(
            container()
                .layout(
                    LayoutStyle::flex_column()
                        .fill_width()
                        .shrink(0.0)
                        .padding_x(XL + L),
                )
                .children_reactive({
                    let strip = app.clone();
                    move || {
                        let (cols, _rows) = strip.viewport.get();
                        vec![chat::panels::status_bar(
                            strip.clone(),
                            chat::canvas::inner_cols(usize::from(cols)),
                        )]
                    }
                }),
        )
        .child(
            container()
                .layout(LayoutStyle::block().fill_width().shrink(0.0))
                .children_reactive({
                    let notice = input.clone();
                    let active = sigil::reactive::derived(move || chat::panels::band(&notice));
                    let notice = input.clone();
                    move || {
                        active
                            .get()
                            .map(|band| vec![column(notice_bar(notice.clone(), band))])
                            .unwrap_or_default()
                    }
                }),
        )
        .child(
            container()
                .layout(LayoutStyle::block().fill_width().shrink(0.0))
                .children_reactive(move || {
                    vec![column(composer::composer(
                        input.clone(),
                        input.view_edit.get(),
                    ))]
                }),
        )
        .child(
            container()
                .layout(LayoutStyle::block().fill_width().shrink(0.0))
                .children_reactive({
                    let bar = app.clone();
                    move || {
                        drop(bar.status.get());
                        vec![column(status::meters(bar.clone()))]
                    }
                }),
        )
        .children_reactive(move || {
            if blank.get() {
                vec![container().layout(LayoutStyle::block().grow(1.0).min_size(0.0, 0.0))]
            } else {
                Vec::new()
            }
        })
}

fn workspace_name(app: &App) -> String {
    let workspace =
        title::active(app).map_or_else(|| app.workspace.clone(), |entry| entry.workspace);
    crate::worktree::display_name(&workspace)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| env!("CARGO_PKG_NAME").to_string())
}

fn empty_home() -> NodeBuilder {
    container().layout(
        LayoutStyle::flex_column()
            .fill_width()
            .fill_height()
            .grow(1.0)
            .min_size(0.0, 0.0),
    )
}

fn starters(app: &App) -> NodeBuilder {
    let mut actions = container().layout(
        LayoutStyle::flex_row()
            .gap(S)
            .align_items(AlignItems::Center)
            .justify_content(JustifyContent::Center),
    );
    if app.model.get().snapshot.unconfigured {
        let providers = app.clone();
        actions = actions.child(
            button(ADD_PROVIDER)
                .primary()
                .size(ControlSize::Compact)
                .activate(move || providers.open_providers()),
        );
        return home(app, actions);
    }
    let explain = app.clone();
    actions = actions.child(
        button(EXPLAIN)
            .size(ControlSize::Compact)
            .activate(move || {
                explain.edit(EXPLAIN);
                explain.submit();
            }),
    );
    let current = app.model.peek().session.clone();
    if let Some(last) = session::previous_for(&app.workspace, &current)
        .ok()
        .flatten()
    {
        let resume = app.clone();
        actions = actions.child(
            button("Continue last session")
                .size(ControlSize::Compact)
                .activate(move || {
                    resume.open_session(crate::tui::hub::Open::Resume(last.id.clone()))
                }),
        );
    }
    home(app, actions)
}

fn home(app: &App, actions: NodeBuilder) -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .padding_y(XL)
                .gap(L)
                .align_items(AlignItems::Center),
        )
        .child(
            sigil::prelude::text(workspace_name(app))
                .nowrap()
                .font_size(26.0)
                .font_weight(600)
                .text_color(Token::OnSurface),
        )
        .child(actions)
}

fn notice_bar(app: App, band: chat::panels::Band) -> NodeBuilder {
    match band {
        chat::panels::Band::Approval { detail, persist } => {
            chat::panels::approval_band(app, usize::MAX, detail, persist)
        }
        chat::panels::Band::Form { lines, review } => {
            chat::form::form_band(app, usize::MAX, lines, review)
        }
        chat::panels::Band::Attached(count) => {
            notice_band(chat::panels::attached_note(&app, count), Tone::Info)
        }
    }
}
