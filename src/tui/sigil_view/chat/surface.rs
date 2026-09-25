use std::rc::Rc;

use sigil::prelude::{
    AlignItems, ColorAtom, Cx, IntoNode, LayoutBuilderExt, LayoutStyle, NodeBuilder, NodeStyleExt,
    Style, classes, container, defer, derived_eq, effect, empty_state, label, node, signal, spacer,
    untrack, virtual_list,
};

use super::composer;
use super::entry;
use super::footer;
use super::panels;
use super::todos;
use super::{App, Entry, canvas, cell_width};
use crate::prefs::Prefs;
use crate::tui::{ToolState, short_session};

pub(in crate::tui::sigil_view) fn layer(
    app: App,
    cols: u16,
    editor: impl IntoNode + 'static,
) -> NodeBuilder {
    let width = canvas::inner_cols(usize::from(cols));
    let transcript_app = app.clone();
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .fill_height()
                .min_size(0.0, 0.0)
                .gap(0.0),
        )
        .child(canvas::gutter(header(app.clone(), width)))
        .child(node(move |cx: &mut Cx| {
            let empty = || empty_state("Summonr", "ask anything · /login · @file · !cmd · /help");
            cx.add(transcript(
                transcript_app.clone(),
                (canvas::GUTTER, canvas::cell().h),
                empty,
            ))
        }))
        .child(canvas::gutter(panels::agents(app.clone(), width)))
        .child(todos::widget(app.clone(), cols))
        .child(canvas::gutter(panels::bands(app.clone(), width)))
        .child(composer::widget(app.clone(), width, editor))
        .child(canvas::gutter(footer::footer_bar(app, width)))
}

#[derive(Clone, Copy, PartialEq)]
enum Part {
    Brand,
    Model,
    Pick,
    Relay,
    Session,
    Mode,
}

const DROP_ORDER: [Part; 4] = [Part::Relay, Part::Session, Part::Pick, Part::Mode];

fn fit(parts: &mut Vec<(Part, String)>, width: usize) {
    let used = |parts: &[(Part, String)]| {
        parts
            .iter()
            .map(|(_, text)| cell_width(text) + 1)
            .sum::<usize>()
    };
    for drop in DROP_ORDER {
        if used(parts) <= width {
            return;
        }
        parts.retain(|(part, _)| *part != drop);
    }
    let over = used(parts).saturating_sub(width);
    if let Some((_, text)) = parts.iter_mut().find(|(part, _)| *part == Part::Model) {
        *text = canvas::ellipsize(text, cell_width(text).saturating_sub(over));
    }
}

fn header(app: App, width: usize) -> NodeBuilder {
    let update = app.clone();
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .align_items(AlignItems::Center)
                .shrink(0.0)
                .gap(8.0),
        )
        .children_reactive(move || {
            let model = update.model.get();
            let snapshot = &model.snapshot;
            let (mode, mode_class) = if snapshot.plan {
                ("plan", classes!("text-xs text-accent"))
            } else if snapshot.work {
                ("work", classes!("text-xs text-warn-11"))
            } else if snapshot.goal {
                ("goal", classes!("text-xs text-error-11"))
            } else {
                ("auto", classes!("text-xs text-success-11"))
            };
            let mut parts = vec![
                (Part::Brand, "summonr".to_owned()),
                (Part::Model, snapshot.model.clone()),
            ];
            if let Some(pick) = model.pick {
                parts.push((Part::Pick, format!("({pick})")));
            }
            if let Some(relay) = snapshot.relay.as_deref().filter(|relay| !relay.is_empty()) {
                parts.push((Part::Relay, format!("· {relay}")));
            }
            parts.push((
                Part::Session,
                format!("ses {}", short_session(&model.session)),
            ));
            parts.push((Part::Mode, mode.to_owned()));
            fit(&mut parts, width);
            let mut row = Vec::new();
            let mut spaced = false;
            for (part, text) in parts {
                if !spaced && matches!(part, Part::Session | Part::Mode) {
                    spaced = true;
                    row.push(spacer());
                }
                let shown = label(text);
                let shown = match part {
                    Part::Brand => shown
                        .class(classes!("text-sm font-bold"))
                        .style(Style::new().text_color(ColorAtom::Rgba8(canvas::palette().accent))),
                    Part::Model => shown.class(classes!("text-sm text-gray-12")),
                    Part::Mode => shown.class(mode_class),
                    Part::Pick | Part::Relay | Part::Session => {
                        shown.class(classes!("text-xs text-gray-11"))
                    }
                };
                row.push(
                    container()
                        .layout(LayoutStyle::block().shrink(0.0))
                        .child(shown),
                );
            }
            row
        })
}

type Content = (usize, u8, bool, Prefs);

fn content(entry: &Entry, live: bool, prefs: &Prefs) -> Content {
    let (size, state) = match entry {
        Entry::Assistant { text, done } => (text.len(), u8::from(*done)),
        Entry::Thinking(text) => (text.len(), 0),
        Entry::Tool { state, output, .. } => (
            output.len(),
            u8::from(!matches!(state, ToolState::Running))
                + u8::from(matches!(state, ToolState::Failed)),
        ),
        Entry::User(_)
        | Entry::Receipt(_)
        | Entry::Verify { .. }
        | Entry::Mail { .. }
        | Entry::Aborted(_)
        | Entry::System(_)
        | Entry::Error(_)
        | Entry::Chart(_)
        | Entry::Image { .. } => (0, 0),
    };
    (size, state, live, prefs.clone())
}

fn row(app: App, index: usize, spacing: (f32, f32)) -> NodeBuilder {
    let model = app.model;
    let version = derived_eq(move || {
        model.with(|model| {
            let live = model.busy_since.is_some() && index + 1 == model.entries.len();
            model
                .entries
                .get(index)
                .map(|entry| content(entry, live, &model.prefs))
        })
    });
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .min_size(0.0, 0.0)
                .padding_x(spacing.0)
                .padding_bottom(spacing.1),
        )
        .children_reactive(move || {
            version.track();
            untrack(|| {
                let (entry, live, prefs) = model.with(|model| {
                    (
                        model.entries.get(index).cloned(),
                        model.busy_since.is_some() && index + 1 == model.entries.len(),
                        model.prefs.clone(),
                    )
                });
                entry
                    .map(|entry| vec![entry::row(entry, live, &prefs, &app)])
                    .unwrap_or_default()
            })
        })
}

pub(in crate::tui::sigil_view) fn transcript(
    app: App,
    spacing: (f32, f32),
    empty: fn() -> NodeBuilder,
) -> NodeBuilder {
    let model = app.model;
    let keys = derived_eq(move || {
        model.with(|model| {
            let stamp = model.entries.stamp;
            Rc::new(
                (0..model.entries.len())
                    .map(|index| (stamp << 32) ^ index as u64)
                    .collect::<Vec<u64>>(),
            )
        })
    });
    let jump = signal(0_usize);
    let target = app.search.target;
    effect(move || {
        if let Some(index) = target.get() {
            jump.set(index);
            defer(move || target.set(None));
        }
    });
    let empty_now = derived_eq(move || keys.get().is_empty());
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .grow(1.0)
                .min_size(0.0, 0.0),
        )
        .children_reactive(move || {
            if empty_now.get() {
                return vec![empty()];
            }
            jump.set(keys.peek().len().saturating_sub(1));
            let live = app.clone();
            vec![
                container()
                    .layout(
                        LayoutStyle::flex_column()
                            .fill_width()
                            .grow(1.0)
                            .min_size(0.0, 0.0),
                    )
                    .child(
                        virtual_list(keys, move |index, _: &u64| {
                            row(live.clone(), index, spacing)
                        })
                        .key_by(|_, key| *key)
                        .measured()
                        .estimate(canvas::cell().h * 2.0)
                        .viewport_fill()
                        .scroll_bind(app.scroll_y)
                        .max_scroll_bind(app.max_scroll)
                        .content_height_bind(app.content_height)
                        .auto_pin_to_bottom(true)
                        .follow(jump),
                    ),
            ]
        })
}
