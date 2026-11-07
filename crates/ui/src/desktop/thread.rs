use sigil::prelude::Paint;
use sigil::prelude::Widget;
use sigil::prelude::{
    AlignItems, ControlSize, JustifyContent, LayoutStyle, LogicalKey, ModifiersState, NodeBuilder,
    Shortcut, Token, Tone, button, container, icon_button, node, notice_band, spacer,
};

use super::{App, chat};

mod ask;
mod busy;
mod composer;
mod model_menu;
mod place;

/// The control that adds a repository.
pub(super) const ADD_REPOSITORY: &str = place::ELSEWHERE;
#[cfg(test)]
pub(super) const fn place_branch() -> &'static str {
    place::BRANCH
}
mod mentions;
mod queue;
pub(crate) mod status;
mod strip;
pub(super) use strip::header;

use super::style::{BODY, GUTTER, L, LINE, M, S, Text, XL, XS};
const COLUMN: f32 = chat::surface::READING;
const CONNECT: &str = "Connect a model";

pub(super) fn quiet(
    content: impl Into<sigil::prelude::TextContent>,
) -> sigil::prelude::TextBuilder {
    super::style::text(Text::Meta, content).nowrap()
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
    let empty = {
        let watch = app.clone();
        sigil::reactive::derived(move || {
            watch.overlay().is_none() && watch.model.get().entries.only_notes().is_some()
        })
    };
    container()
        .layout(
            LayoutStyle::flex_column()
                .grow(1.0)
                .fill_width()
                .fill_height()
                .min_size(0.0, 0.0)
                .padding_bottom(M)
                .position(sigil::prelude::Position::Relative)
                .align_items(AlignItems::Stretch),
        )
        .bg(super::style::CANVAS)
        .child(super::sky::behind(&app, empty))
        .child(header(&app))
        .child(
            container()
                .layout(
                    LayoutStyle::flex_column()
                        .fill_width()
                        .grow(1.0)
                        .min_size(0.0, 0.0),
                )
                .children_reactive(move || {
                    if empty.get() {
                        let app = transcript_app.clone();
                        return vec![sigil::reactive::untrack(|| hero(&app))];
                    }
                    let app = transcript_app.clone();
                    vec![sigil::reactive::untrack(|| {
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
                                chat::surface::transcript(app.clone(), (L, L), empty_home)
                            }))
                    })]
                }),
        )
        .child(
            container()
                .layout(LayoutStyle::flex_column().fill_width().shrink(0.0))
                .children_reactive({
                    let strip = app.clone();
                    move || {
                        if strip.model.get().busy_since.is_none()
                            || chat::panels::band(&strip).is_some()
                        {
                            return Vec::new();
                        }
                        vec![column(busy::row(&strip))]
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
                    if empty.get() {
                        return Vec::new();
                    }
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

fn hero(app: &App) -> NodeBuilder {
    let draft = app.clone();
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .grow(1.0)
                .min_size(0.0, 0.0)
                .padding_x(XL)
                .padding_bottom(XL * 2.0)
                .align_items(AlignItems::Center)
                .justify_content(JustifyContent::Center),
        )
        .child(
            container()
                .layout(
                    LayoutStyle::flex_column()
                        .fill_width()
                        .max_width(COLUMN)
                        .gap(L)
                        .align_items(AlignItems::Stretch),
                )
                .child(
                    container()
                        .layout(LayoutStyle::flex_column().fill_width())
                        .children_reactive(move || {
                            vec![composer::composer(draft.clone(), draft.view_edit.get())]
                        }),
                )
                .child({
                    let offers = app.clone();
                    container().children_reactive(move || vec![starters(&offers)])
                }),
        )
}

fn starters(app: &App) -> NodeBuilder {
    let row = container().layout(
        LayoutStyle::flex_row()
            .gap(S)
            .align_items(AlignItems::Center)
            .flex_wrap(sigil::prelude::FlexWrap::Wrap),
    );
    if app.model.get().snapshot.unconfigured {
        return setup(app, row);
    }
    row
}

fn setup(app: &App, row: NodeBuilder) -> NodeBuilder {
    let local = app.window.local.get();
    let row = local.iter().enumerate().fold(row, |row, (index, found)| {
        let (choose, found) = (app.clone(), found.clone());
        let base = button(format!("Use {}", found.name)).size(ControlSize::Compact);
        let base = if index == 0 {
            base.primary()
        } else {
            base.neutral().soft()
        };
        row.child(base.activate(move || choose.use_local(&found)))
    });
    let providers = app.clone();
    let connect = button(CONNECT).size(ControlSize::Compact);
    let connect = if local.is_empty() {
        connect.primary()
    } else {
        connect.neutral().soft()
    };
    row.child(connect.activate(move || providers.open_providers()))
}

fn notice_bar(app: App, band: chat::panels::Band) -> NodeBuilder {
    if let Some(card) = ask::card(&app, &band) {
        return card;
    }
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
        chat::panels::Band::Held => held(app),
    }
}

/// The note that another window holds the chat, with Take over and Not now.
fn held(app: App) -> NodeBuilder {
    let (take, dismiss) = (app.clone(), app);
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .gap(S)
                .padding_left(GUTTER)
                .padding_bottom(S)
                .align_items(AlignItems::Center),
        )
        .child(
            container()
                .layout(LayoutStyle::block().grow(1.0).min_size(0.0, 0.0))
                .child(super::style::text(Text::Meta, chat::panels::HELD)),
        )
        .child(
            button("Not now")
                .ghost()
                .size(ControlSize::Compact)
                .activate(move || dismiss.held.set(None)),
        )
        .child(
            button("Take over")
                .primary()
                .size(ControlSize::Compact)
                .activate(move || take.take_over()),
        )
        .into()
}
