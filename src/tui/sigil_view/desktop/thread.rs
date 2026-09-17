use crate::tui::context;
use sigil::prelude::{
    AlignItems, IntoNode, JustifyContent, LayoutBuilderExt, LayoutReactiveExt, LayoutStyle,
    LogicalKey, ModifiersState, NodeBuilder, NodeEventsExt, NodeStyleExt, ObjectFit, Shortcut,
    Tone, WidgetLayoutExt, badge, chart, classes, container, effect, image, label, scroll, signal,
    text, text_edit, text_live,
};

use super::super::app::App;
use super::super::model::Entry;
use super::super::pane::Pane;
use super::{hairline, hit, icons, open, spacer};
use crate::prefs::ToolOutput;
use crate::session;

pub(super) fn main_col(app: App) -> NodeBuilder {
    let title = app.clone();
    let input = app.clone();
    container()
        .layout(
            LayoutStyle::flex_column()
                .grow(1.0)
                .fill_height()
                .min_size(0.0, 0.0),
        )
        .class(classes!("bg-gray-1"))
        .child(
            container()
                .layout(LayoutStyle::block().fill_width().shrink(0.0))
                .children_reactive(move || {
                    let _updates = (title.panes.get(), title.layout.get());
                    vec![title_bar(title.clone())]
                }),
        )
        .child(transcript(app.clone()))
        .child(
            container()
                .layout(LayoutStyle::block().fill_width().shrink(0.0))
                .children_reactive(move || vec![composer(input.clone(), input.view_edit.get())]),
        )
        .child(
            container()
                .layout(LayoutStyle::block().fill_width().shrink(0.0))
                .children_reactive(move || {
                    drop(app.status.get());
                    vec![status_strip(app.clone())]
                }),
        )
}

fn title_bar(app: App) -> NodeBuilder {
    let workspace = app
        .workspace
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let run = app.clone();
    let (added, removed) = {
        let model = app.model.borrow();
        (model.diff.added(), model.diff.removed())
    };
    let badge_app = app.clone();
    let options = app.clone();
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .height(52.0)
                .padding_x(20.0)
                .gap(8.0)
                .align_items(AlignItems::Center),
        )
        .child(
            label(format!(
                "session {}",
                session::short(&app.model.borrow().session)
            ))
            .class(classes!("text-sm font-medium text-gray-12")),
        )
        .child(label(workspace).class(classes!("text-sm text-gray-11")))
        .child(spacer())
        .child(
            hit("Options".to_string(), false, move || {
                options.toggle_options()
            })
            .layout(
                LayoutStyle::flex_row()
                    .fixed_size(28.0, 28.0)
                    .align_items(AlignItems::Center)
                    .justify_content(JustifyContent::Center),
            )
            .child(icons::options()),
        )
        .child(
            hit("Run".to_string(), false, move || run.submit())
                .layout(
                    LayoutStyle::flex_row()
                        .fixed_size(28.0, 28.0)
                        .align_items(AlignItems::Center)
                        .justify_content(JustifyContent::Center),
                )
                .child(icons::play()),
        )
        .child(hairline())
        .child(
            hit("Changes".to_string(), open(&app, Pane::Diff), move || {
                badge_app.toggle_pane(Pane::Diff);
            })
            .layout(
                LayoutStyle::flex_row()
                    .height(28.0)
                    .padding_x(8.0)
                    .gap(6.0)
                    .align_items(AlignItems::Center),
            )
            .class(classes!("rounded-md border border-gray-6"))
            .child(icons::diffs())
            .child(label(format!("+{added}")).class(classes!("text-xs text-success-11")))
            .child(label(format!("-{removed}")).class(classes!("text-xs text-error-11"))),
        )
}

fn transcript(app: App) -> NodeBuilder {
    let live = app.clone();
    let bounds = signal((0.0, 0.0, 0.0, 0.0));
    let extent = app.extent;
    let body = container()
        .bounds_reactive(bounds)
        .on_added(move |_, _| {
            effect(move || {
                let height = bounds.get().3;
                if height > 0.0 {
                    extent.set(height);
                }
            });
        })
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .padding_x(36.0)
                .padding_y(20.0)
                .gap(16.0),
        )
        .children_reactive(move || {
            let _revision = live.panes.get();
            let entries: Vec<Entry> = live.model.borrow().entries.to_vec();
            let tools = live.model.borrow().prefs.tools;
            let mut column = container().layout(LayoutStyle::flex_column().fill_width().gap(16.0));
            if entries.is_empty() {
                column = column.child(
                    label("ask anything — @file attaches · !cmd runs · /help lists")
                        .class(classes!("text-sm text-gray-11")),
                );
            } else {
                for (index, entry) in entries.into_iter().enumerate() {
                    if live.search.target.peek() != Some(index) {
                        column = column.child(row(entry, tools));
                        continue;
                    }
                    let measured = signal((0.0, 0.0, 0.0, 0.0));
                    let target = live.search.target;
                    let scroll = live.scroll_y;
                    column = column.child(row(entry, tools).bounds_reactive(measured).on_added(
                        move |_, _| {
                            effect(move || {
                                let (_, y, _, height) = measured.get();
                                let (_, top, _, body_height) = bounds.get();
                                if target.peek() == Some(index) && height > 0.0 && body_height > 0.0
                                {
                                    scroll.set((y - top).max(0.0));
                                    target.set(None);
                                }
                            });
                        },
                    ));
                }
            }
            vec![column]
        });
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .grow(1.0)
                .min_size(0.0, 0.0),
        )
        .child(
            scroll(body, app.extent)
                .fill()
                .scroll_bind(app.scroll_y)
                .auto_pin_to_bottom(app.search.target.peek().is_none()),
        )
}

fn row(entry: Entry, tools: ToolOutput) -> NodeBuilder {
    match entry {
        Entry::User(body) => container()
            .layout(
                LayoutStyle::flex_row()
                    .fill_width()
                    .justify_content(JustifyContent::End),
            )
            .child(
                container()
                    .layout(
                        LayoutStyle::flex_row()
                            .padding_x(14.0)
                            .padding_y(8.0)
                            .max_size(560.0, f32::INFINITY),
                    )
                    .class(classes!("rounded-lg bg-gray-4"))
                    .child(text(body).class(classes!("text-sm text-gray-12"))),
            ),
        Entry::Assistant { text: body, .. } => prose(&body, "text-sm text-gray-12"),
        Entry::Thinking(body) => prose(&body, "text-xs text-gray-11"),
        Entry::Receipt(body) => prose(&body, "text-xs text-gray-11"),
        Entry::Tool {
            name, arg, output, ..
        } => container()
            .layout(LayoutStyle::flex_column().fill_width().gap(4.0))
            .child(
                container()
                    .layout(
                        LayoutStyle::flex_row()
                            .gap(8.0)
                            .align_items(AlignItems::Center),
                    )
                    .child(badge(name, Tone::Neutral))
                    .child(
                        label(arg)
                            .ellipsize()
                            .layout(LayoutStyle::block().grow(1.0).min_size(0.0, 14.0))
                            .class(classes!("text-xs font-mono text-gray-11")),
                    ),
            )
            .child(text(tools.text(&output)).class(classes!("text-xs font-mono text-gray-11"))),
        Entry::Verify { command, passed } => wrap(
            label(format!("{} {command}", if passed { "✓" } else { "✗" })).class(if passed {
                classes!("text-xs text-success-11")
            } else {
                classes!("text-xs text-error-11")
            }),
        ),
        Entry::Mail { from, text: body } => {
            prose(&format!("{from}: {body}"), "text-sm text-info-11")
        }
        Entry::Aborted(reason) | Entry::Error(reason) => {
            wrap(text(reason).class(classes!("text-xs text-error-11")))
        }
        Entry::System(body) => wrap(text(body).class(classes!("text-xs text-gray-11"))),
        Entry::Chart(rounds) => wrap(
            chart(rounds.iter().map(|round| round.total() as f32))
                .height(72.0)
                .width(280.0),
        ),
        Entry::Image {
            source,
            label: name,
        } => {
            let height = source.height() as f32;
            let width = source.width() as f32;
            container()
                .layout(LayoutStyle::flex_column().fill_width().gap(6.0))
                .child(
                    image(source)
                        .alt(name.clone())
                        .fit(ObjectFit::Contain)
                        .layout(
                            LayoutStyle::block()
                                .fill_width()
                                .max_size(width, height)
                                .height(height),
                        ),
                )
                .child(label(name).class(classes!("text-xs text-gray-11")))
        }
    }
}

fn wrap(child: impl IntoNode + 'static) -> NodeBuilder {
    container()
        .layout(LayoutStyle::block().fill_width().min_size(0.0, 0.0))
        .child(child)
}

fn prose(body: &str, class: &'static str) -> NodeBuilder {
    let mut column = container().layout(LayoutStyle::flex_column().fill_width().gap(6.0));
    for para in body.split('\n') {
        let mut row = container()
            .layout(
                LayoutStyle::flex_row()
                    .fill_width()
                    .gap(4.0)
                    .align_items(AlignItems::Center)
                    .min_size(0.0, 0.0),
            )
            .class(classes!("flex-wrap"));
        let mut rest = para;
        while let Some(start) = rest.find('`') {
            if start > 0 {
                row = row.child(text(rest[..start].to_string()).class(class));
            }
            rest = &rest[start + 1..];
            let Some(end) = rest.find('`') else {
                break;
            };
            row = row.child(badge(rest[..end].to_string(), Tone::Neutral));
            rest = &rest[end + 1..];
        }
        if !rest.is_empty() {
            row = row.child(text(rest.to_string()).class(class));
        }
        column = column.child(row);
    }
    column
}

fn composer(app: App, editing: bool) -> NodeBuilder {
    let submit = app.clone();
    let send = app.clone();
    let changed = app.clone();
    let paste = app.clone();
    let paste_meta = app.clone();
    let (value, caret) = if editing {
        (app.view_text, app.view_caret)
    } else {
        (app.draft, app.caret)
    };
    let model = app.clone();
    container()
        .key_capture(Shortcut::ctrl(LogicalKey::character("v")), move |event| {
            if paste.clipboard() {
                event.stop_propagation();
            }
        })
        .key_capture(
            Shortcut::new(LogicalKey::character("v"), ModifiersState::META),
            move |event| {
                if paste_meta.clipboard() {
                    event.stop_propagation();
                }
            },
        )
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .padding_x(24.0)
                .padding_y(12.0)
                .shrink(0.0),
        )
        .child(
            container()
                .layout(
                    LayoutStyle::flex_column()
                        .fill_width()
                        .padding(16.0)
                        .gap(12.0),
                )
                .class(classes!("rounded-lg border border-gray-6 bg-gray-1"))
                .child(
                    text_edit(value)
                        .caret_position(caret)
                        .placeholder(if editing {
                            "editing the file — ctrl+s saves, esc leaves"
                        } else {
                            "ask for changes — @file attaches, /help lists"
                        })
                        .change(move |draft| {
                            if !editing {
                                changed.change(draft);
                            }
                        })
                        .submit(move |_| submit.submit())
                        .height(72.0)
                        .class(classes!("border-0 bg-gray-1")),
                )
                .child(
                    container()
                        .layout(
                            LayoutStyle::flex_row()
                                .fill_width()
                                .align_items(AlignItems::Center)
                                .gap(8.0),
                        )
                        .child(
                            text_live(move || {
                                drop(model.header.get());
                                model.model.borrow().snapshot.model.clone()
                            })
                            .class(classes!("text-sm text-gray-12")),
                        )
                        .child(spacer())
                        .child(
                            hit("Send".to_string(), false, move || send.submit())
                                .layout(
                                    LayoutStyle::flex_row()
                                        .fixed_size(28.0, 28.0)
                                        .shrink(0.0)
                                        .align_items(AlignItems::Center)
                                        .justify_content(JustifyContent::Center),
                                )
                                .class(classes!("rounded-full bg-gray-12"))
                                .child(icons::send()),
                        ),
                ),
        )
}

fn status_strip(app: App) -> NodeBuilder {
    let (plan, context, tokens, cached) = {
        let model = app.model.borrow();
        let cached = (model.cached_tokens > 0 && model.prompt_tokens > 0)
            .then(|| model.cached_tokens.saturating_mul(100) / model.prompt_tokens);
        (
            model.snapshot.plan,
            context(model.usage.last(), model.snapshot.context_window, false),
            model.prompt_tokens.saturating_add(model.completion_tokens),
            cached,
        )
    };
    let branch = app.git_branch.clone().unwrap_or_default();
    let auto = app.clone();
    let planned = app.clone();
    let mut strip = container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .height(32.0)
                .padding_x(24.0)
                .align_items(AlignItems::Center)
                .gap(8.0)
                .shrink(0.0),
        )
        .child(
            container()
                .layout(
                    LayoutStyle::flex_row()
                        .height(26.0)
                        .padding(2.0)
                        .gap(2.0)
                        .align_items(AlignItems::Center),
                )
                .class(classes!("rounded-md bg-gray-3"))
                .child(seg("Auto", !plan, move || {
                    if auto.model.borrow().snapshot.plan {
                        auto.toggle_mode();
                    }
                }))
                .child(seg("Plan", plan, move || {
                    if !planned.model.borrow().snapshot.plan {
                        planned.toggle_mode();
                    }
                })),
        )
        .child(hairline())
        .child(metric("context", context))
        .child(hairline())
        .child(metric(
            "session",
            format!("{} tokens", crate::provider::compact(tokens)),
        ));
    if let Some(cached) = cached {
        strip =
            strip.child(label(format!("{cached}% cached")).class(classes!("text-xs text-gray-11")));
    }
    strip
        .child(spacer())
        .child(icons::branch())
        .child(label(branch).class(classes!("text-xs text-gray-11")))
}

fn metric(name: &'static str, value: String) -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_row()
                .align_items(AlignItems::Center)
                .gap(4.0),
        )
        .child(label(name).class(classes!("text-xs text-gray-11")))
        .child(label(value).class(classes!("text-xs font-mono text-gray-12")))
}

fn seg(title: &'static str, on: bool, act: impl Fn() + 'static) -> NodeBuilder {
    hit(title.to_string(), false, act)
        .layout(
            LayoutStyle::flex_row()
                .height(22.0)
                .padding_x(10.0)
                .align_items(AlignItems::Center)
                .justify_content(JustifyContent::Center),
        )
        .class(classes!("rounded-sm"))
        .class_when(signal(on), "bg-gray-1")
        .child(
            label(title)
                .class(classes!("text-xs text-gray-11"))
                .class_when(signal(on), "text-gray-12"),
        )
}
