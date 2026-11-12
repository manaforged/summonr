use sigil::prelude::Widget;
use sigil::prelude::{
    AlignItems, JustifyContent, MouseButton, NodeBuilder, Paint, PointerEvent, Radius, Space,
    container, markdown, signal,
};

use super::super::App;
use super::super::style::{
    BODY, DANGER, L, LINE, M, OUTLINE, RECESSED, S, TINT, Text, XS, text as styled,
};
use crate::chat::entry::reply::{self, Part, parts};
use summonr_agent::prefs::Visibility;

const FENCE: &str = "```";
const BUBBLE: f32 = 520.0;

pub(super) fn copyable(app: &App, node: NodeBuilder, body: String) -> NodeBuilder {
    let app = app.clone();
    sigil::widgets::context_menu(node, move || {
        let (app, body) = (app.clone(), body.clone());
        vec![sigil::widgets::MenuItem::new("Copy").activate(move || {
            app.copy.set(Some(body.clone()));
        })]
    })
}

pub(super) fn user(app: &App, body: String) -> NodeBuilder {
    let text = body.clone();
    let bubble = sigil::prelude::row()
        .gap(sigil::prelude::Space::None)
        .align_items(sigil::prelude::AlignItems::Stretch)
        .fill_width()
        .min_width(0.0)
        .min_height(0.0)
        .justify_content(JustifyContent::End)
        .child(
            container()
                .padding_x(L)
                .padding_y(S)
                .max_width(BUBBLE)
                .min_width(0.0)
                .min_height(0.0)
                .background(TINT)
                .radius(Radius::Lg)
                .child(styled(Text::Body, body).line_height(LINE)),
        );
    copyable(app, bubble, text)
}

fn code_block(app: &App, lang: String, body: String) -> NodeBuilder {
    let copy = app.clone();
    let copied = body.clone();
    let head = sigil::prelude::row()
        .gap(sigil::prelude::Space::None)
        .fill_width()
        .padding_left(M)
        .padding_right(XS)
        .padding_top(XS)
        .align_items(AlignItems::Center)
        .justify_content(JustifyContent::SpaceBetween)
        .child(styled(Text::Meta, lang.clone()))
        .child(
            sigil::prelude::button(sigil::icons::Icon::Copy).activate(move || {
                copy.copy.set(Some(copied.clone()));
            })
            .size(sigil::prelude::ControlSize::Compact)
            .accessible("Copy code"),
        );
    sigil::prelude::column()
        .gap(sigil::prelude::Space::None)
        .fill_width()
        .min_width(0.0)
        .min_height(0.0)
        .background(RECESSED)
        .border_width(1)
        .border_color(OUTLINE)
        .radius(Radius::Md)
        .child(head)
        .child(markdown(signal(format!("{FENCE}{lang}\n{body}\n{FENCE}"))))
}

pub(super) fn assistant(body: &str, app: &App) -> NodeBuilder {
    let gap = Space::raw(
        f32::from(app.model.get().prefs.spacing),
        "the user's paragraph spacing",
    );
    let mut column = sigil::prelude::column()
        .font_size(BODY, "body text for replies")
        .fill_width()
        .gap(gap)
        .min_width(0.0)
        .min_height(0.0);
    if body.trim().is_empty() {
        return column;
    }
    let layout = app.model.get().prefs.diff_layout;
    for block in parts(body) {
        column = column.child(match block {
            Part::Prose(text) => container()
                .fill_width()
                .min_width(0.0)
                .min_height(0.0)
                .line_height(LINE)
                .child(markdown(signal(text)).gap(gap)),
            Part::Code { lang, body, closed } => {
                if !closed && let Some(card) = crate::desktop::entry::diff_card(app, &body) {
                    card
                } else {
                    code_block(app, lang, body)
                }
            }
            Part::Shown(shown) => reply::paint(shown, app, layout),
        });
    }
    column
}

pub(super) fn thinking(body: &str, visibility: Visibility, live: bool, app: &App) -> NodeBuilder {
    if body.trim().is_empty() {
        return container();
    }
    let toggle = app.clone();
    let open = visibility == Visibility::Expanded;
    let next = if open {
        Visibility::Collapsed
    } else {
        Visibility::Expanded
    };
    let words = summonr_core::text::plural(body.split_whitespace().count(), "word");
    let head = if live {
        "Thinking…".to_string()
    } else {
        format!("Thought · {words}")
    };
    let row = sigil::prelude::row()
        .gap(S)
        .align_items(AlignItems::Center)
        .child(styled(Text::Meta, if open { "▾" } else { "▸" }))
        .child(styled(Text::Meta, head));
    let mut column = sigil::prelude::column()
        .fill_width()
        .gap(XS)
        .pointer_up(move |event: &mut PointerEvent| {
            if event.button == Some(MouseButton::Left) {
                toggle.set_visibility(|prefs| &mut prefs.reasoning, Some(next));
                event.stop_propagation();
            }
        })
        .child(row);
    if open {
        column = column.child(styled(Text::Meta, body.to_string()).line_height(LINE));
    }
    column
}

/// A failure's one line, with a Retry button when `retry` gives the app to retry in.
pub(super) fn error(reason: String, retry: Option<&App>) -> NodeBuilder {
    let row = sigil::prelude::row()
        .fill_width()
        .gap(S)
        .min_width(0.0)
        .min_height(0.0)
        .align_items(AlignItems::Center)
        .child(styled(Text::Label, reason).text_color(DANGER));
    match retry {
        Some(app) => {
            let app = app.clone();
            row.child(
                sigil::prelude::button("Retry")
                    .ghost()
                    .size(sigil::prelude::ControlSize::Compact)
                    .activate(move || app.retry()),
            )
        }
        None => row,
    }
}

/// The first line of a failure.
pub(super) fn first_line(reason: &str) -> String {
    reason.lines().next().unwrap_or_default().to_owned()
}

/// The part of a status line before its key hints.
pub(super) fn first_part(reason: &str) -> String {
    reason.split(" · ").next().unwrap_or_default().to_owned()
}

#[cfg(test)]
#[path = "message_tests.rs"]
mod tests;
