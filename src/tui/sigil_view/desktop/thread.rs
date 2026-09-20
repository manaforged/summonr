use crate::tui::context;
use sigil::prelude::{
    AlignItems, Cx, JustifyContent, LayoutBuilderExt, LayoutReactiveExt, LayoutStyle, LogicalKey,
    ModifiersState, NodeBuilder, NodeEventsExt, NodeStyleExt, Shortcut, Tone, WidgetLayoutExt,
    button, classes, container, effect, empty_state, hairline, icon_button, label, node,
    notice_band, scroll, signal, spacer, text_edit, text_live,
};

use super::icons;
use super::{App, Entry, Pane, chat};
use crate::session;

mod composer;
pub(in crate::tui::sigil_view) mod status;

pub(super) fn main_col(app: App) -> NodeBuilder {
    let input = app.clone();
    let transcript_app = app.clone();
    container()
        .layout(
            LayoutStyle::flex_column()
                .grow(1.0)
                .fill_width()
                .fill_height()
                .min_size(0.0, 0.0)
                .align_items(AlignItems::Stretch),
        )
        .class(classes!("bg-gray-1"))
        .child(node(move |cx: &mut Cx| cx.add(transcript(transcript_app))))
        .child(
            container()
                .layout(LayoutStyle::block().fill_width().shrink(0.0))
                .children_reactive({
                    let notice = input.clone();
                    move || {
                        let _ticks = notice.ticks.get();
                        vec![notice_bar(notice.clone())]
                    }
                }),
        )
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
                .children_reactive({
                    let bar = app.clone();
                    move || {
                        drop(bar.status.get());
                        vec![status::status_strip(bar.clone())]
                    }
                }),
        )
}

pub(super) fn title_bar(app: App) -> NodeBuilder {
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
            label(if workspace.is_empty() {
                "Summonr".to_string()
            } else {
                workspace
            })
            .class(classes!("text-sm font-medium text-gray-12")),
        )
        .child(
            label(session::short(&app.model.borrow().session).to_string())
                .class(classes!("text-xs text-gray-11")),
        )
        .child(spacer())
        .child(icon_button("···", move || options.toggle_options()).accessible("Options"))
        .child(button("Run").primary().activate(move || run.submit()))
        .child(hairline())
        .child(
            button(format!("+{added} −{removed}"))
                .ghost()
                .activate(move || badge_app.toggle_pane(Pane::Diff)),
        )
}

fn empty_home() -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .fill_height()
                .grow(1.0)
                .min_size(0.0, 120.0)
                .align_items(AlignItems::Center)
                .justify_content(JustifyContent::Center),
        )
        .child(empty_state(
            "Summonr",
            "Message the agent. @file attaches · /help lists commands.",
        ))
}

fn notice_bar(app: App) -> NodeBuilder {
    if let Some((detail, persist)) = app.approval.get() {
        let go = app.clone();
        let always = app.clone();
        let stop = app.clone();
        let mut actions = container().layout(
            LayoutStyle::flex_row()
                .gap(8.0)
                .align_items(AlignItems::Center),
        );
        actions = actions.child(
            button("Allow")
                .primary()
                .activate(move || go.approval_answer("y")),
        );
        if persist {
            actions =
                actions.child(button("Always").activate(move || always.approval_answer("always")));
        }
        actions = actions.child(
            button("Deny")
                .destructive()
                .activate(move || stop.approval_answer("n")),
        );
        return notice_band(format!("Needs approval: {detail}"), Tone::Warn)
            .child(label("The agent is waiting. Allow, Always, or Deny."))
            .child(actions);
    }
    if let Some(status) = chat::busy_status(&app.model.borrow()) {
        return notice_band(status, Tone::Neutral);
    }
    container()
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
                .fill_height()
                .grow(1.0)
                .min_size(0.0, 0.0)
                .padding_x(36.0)
                .padding_y(20.0)
                .gap(16.0),
        )
        .children_reactive(move || {
            let _revision = live.panes.get();
            let _ticks = live.ticks.get();
            let entries: Vec<Entry> = live.model.borrow().entries.to_vec();
            let tools = live.model.borrow().prefs.tools;
            if entries.is_empty() {
                return vec![empty_home()];
            }
            let mut column = container().layout(LayoutStyle::flex_column().fill_width().gap(16.0));
            for (index, entry) in entries.into_iter().enumerate() {
                if live.search.target.peek() != Some(index) {
                    column = column.child(chat::entry::row(entry, tools));
                    continue;
                }
                let measured = signal((0.0, 0.0, 0.0, 0.0));
                let target = live.search.target;
                let scroll = live.scroll_y;
                column = column.child(
                    chat::entry::row(entry, tools)
                        .bounds_reactive(measured)
                        .on_added(move |_, _| {
                            effect(move || {
                                let (_, y, _, height) = measured.get();
                                let (_, top, _, body_height) = bounds.get();
                                if target.peek() == Some(index) && height > 0.0 && body_height > 0.0
                                {
                                    scroll.set((y - top).max(0.0));
                                    target.set(None);
                                }
                            });
                        }),
                );
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
