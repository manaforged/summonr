use crate::tui::context;
use sigil::prelude::{
    AlignItems, Cx, IntoNode, JustifyContent, LayoutBuilderExt, LayoutReactiveExt, LayoutStyle,
    LogicalKey, ModifiersState, NodeBuilder, NodeEventsExt, NodeStyleExt, ObjectFit, Shortcut,
    Tone, WidgetLayoutExt, badge, chart, classes, container, effect, image, label, node, scroll,
    signal, text, text_edit, text_live,
};

use super::super::app::App;
use super::super::controls::{hairline, hit, spacer};
use super::super::model::Entry;
use super::super::pane::Pane;
use super::{icons, open};
use crate::prefs::ToolOutput;
use crate::session;

mod composer;
mod entry;
mod status;

pub(super) fn main_col(app: App) -> NodeBuilder {
    let title = app.clone();
    let input = app.clone();
    let transcript_app = app.clone();
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
        .child(node(move |cx: &mut Cx| cx.add(transcript(transcript_app))))
        .child(
            container()
                .layout(LayoutStyle::block().fill_width().shrink(0.0))
                .children_reactive(move || {
                    vec![composer::composer(input.clone(), input.view_edit.get())]
                }),
        )
        .child(
            container()
                .layout(LayoutStyle::block().fill_width().shrink(0.0))
                .children_reactive(move || {
                    drop(app.status.get());
                    vec![status::status_strip(app.clone())]
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
    let viewport = signal((0.0, 0.0));
    let transcript_height = app.transcript_height;
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
                        column = column.child(entry::row(entry, tools));
                        continue;
                    }
                    let measured = signal((0.0, 0.0, 0.0, 0.0));
                    let target = live.search.target;
                    let scroll = live.scroll_y;
                    column =
                        column.child(entry::row(entry, tools).bounds_reactive(measured).on_added(
                            move |_, _| {
                                effect(move || {
                                    let (_, y, _, height) = measured.get();
                                    let (_, top, _, body_height) = bounds.get();
                                    if target.peek() == Some(index)
                                        && height > 0.0
                                        && body_height > 0.0
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
        .size_reactive(viewport)
        .on_added(move |_, _| {
            effect(move || {
                let height = viewport.get().1;
                if height > 0.0 {
                    transcript_height.set(height);
                }
            });
        })
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
