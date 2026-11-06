use sigil::prelude::{
    AlignItems, LayoutStyle, NodeBuilder, Paint, Radius, Signal, Widget, container, dropdown,
    effect, segmented, signal, toggle,
};
use sigil::reactive::{defer, untrack};

use super::super::style::{DIVIDER, L, M, RAISED, S, Text, XS, text};

/// The widest a Settings page grows.
pub(in crate::desktop) const PAGE: f32 = 720.0;
/// The widest a control column grows.
const CONTROL: f32 = 240.0;

/// A titled group of rows in one rounded card, rows split by hairlines.
/// The line under a page whose switches reach new agents only.
pub(in crate::desktop) fn kept_note() -> NodeBuilder {
    crate::desktop::style::text(
        crate::desktop::style::Text::Meta,
        "Agents already working keep their tools",
    )
    .into()
}

pub(in crate::desktop) fn section(title: Option<&str>, rows: Vec<NodeBuilder>) -> NodeBuilder {
    let mut body = container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .align_items(AlignItems::Stretch),
        )
        .bg(RAISED)
        .rounded(Radius::Lg)
        .border_w(1)
        .border_color(DIVIDER);
    for (at, row) in rows.into_iter().enumerate() {
        if at > 0 {
            body = body.child(
                container()
                    .layout(LayoutStyle::block().fill_width().height(1.0))
                    .bg(DIVIDER),
            );
        }
        body = body.child(row);
    }
    let column = container().layout(
        LayoutStyle::flex_column()
            .fill_width()
            .gap(S)
            .align_items(AlignItems::Stretch),
    );
    match title {
        Some(title) => column.child(text(Text::Heading, title)).child(body),
        None => column.child(body),
    }
}

/// One settings row: a label with an optional one-line description, and a control on the right.
pub(in crate::desktop) fn row(
    label: &str,
    about: Option<&str>,
    control: NodeBuilder,
) -> NodeBuilder {
    let words = container()
        .layout(
            LayoutStyle::flex_column()
                .grow(1.0)
                .shrink(1.0)
                .min_size(0.0, 0.0)
                .gap(XS),
        )
        .child(text(Text::Body, label));
    let words = match about {
        Some(about) => words.child(text(Text::Hint, about).wrap()),
        None => words,
    };
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .gap(L)
                .padding_x(L)
                .padding_y(M)
                .align_items(AlignItems::Center),
        )
        .child(words)
        .child(
            container()
                .layout(
                    LayoutStyle::flex_row()
                        .shrink(0.0)
                        .max_width(CONTROL)
                        .align_items(AlignItems::Center),
                )
                .child(control),
        )
}

/// A row with an on/off switch; `flip` runs with the new value.
pub(in crate::desktop) fn switch(
    label: &str,
    about: Option<&str>,
    on: bool,
    flip: impl Fn(bool) + 'static,
) -> NodeBuilder {
    let state = signal(on);
    let flip = std::rc::Rc::new(flip);
    row(
        label,
        about,
        toggle(state)
            .accessible(label)
            .change(move |value| {
                let flip = flip.clone();
                defer(move || flip(value));
            })
            .into(),
    )
}

/// A row with one choice of a few, shown as a segmented control; `pick` runs with the new index.
pub(in crate::desktop) fn choice(
    label: &str,
    about: Option<&str>,
    options: &[&str],
    at: usize,
    pick: impl Fn(usize) + 'static,
) -> NodeBuilder {
    let active = signal(at);
    watch(active, at, pick);
    let control = segmented(
        active,
        options.iter().map(|option| (*option).to_owned()).collect(),
    )
    .label(label);
    row(label, about, control.into())
}

/// A row with one choice of many, shown as a select box; `pick` runs with the new index.
pub(in crate::desktop) fn select(
    label: &str,
    about: Option<&str>,
    options: Vec<String>,
    at: usize,
    pick: impl Fn(usize) + 'static,
) -> NodeBuilder {
    let selected = signal(at);
    let pick = std::rc::Rc::new(pick);
    let control = dropdown(selected, options)
        .label(label)
        .change(move |index| {
            let pick = pick.clone();
            defer(move || pick(index));
        });
    row(
        label,
        about,
        container()
            .layout(LayoutStyle::block().width(CONTROL))
            .child(control),
    )
}

fn watch(active: Signal<usize>, first: usize, pick: impl Fn(usize) + 'static) {
    let pick = std::rc::Rc::new(pick);
    let last = std::rc::Rc::new(std::cell::Cell::new(first));
    effect(move || {
        let now = active.get();
        if now != last.replace(now) {
            let pick = pick.clone();
            untrack(|| defer(move || pick(now)));
        }
    });
}
