use crate::tui::context;
use sigil::prelude::Paint;
use sigil::prelude::{
    AlignItems, JustifyContent, LayoutBuilderExt, LayoutStyle, LogicalKey, ModifiersState,
    NodeBuilder, NodeEventsExt, Shortcut, Tone, WidgetLayoutExt, button, container, empty_state,
    hairline, icon_button, node, notice_band, spacer, text_edit,
};

use super::icons;
use super::{App, Pane, chat};
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
        .bg(sigil::prelude::Token::Gray1)
        .child(node(move || {
            chat::transcript(transcript_app, (36.0, 16.0), empty_home)
        }))
        .child(
            container()
                .layout(
                    LayoutStyle::flex_column()
                        .fill_width()
                        .shrink(0.0)
                        .padding_x(36.0),
                )
                .children_reactive({
                    let strip = app.clone();
                    move || {
                        let (cols, _rows) = strip.viewport.get();
                        vec![chat::panels::agents(
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
                            .map(|band| vec![notice_bar(notice.clone(), band)])
                            .unwrap_or_default()
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
        let model = app.model.get();
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
            sigil::kernel::text(if workspace.is_empty() {
                "Summonr".to_string()
            } else {
                workspace
            })
            .nowrap()
            .text_color(sigil::prelude::Token::OnSurface)
            .font_size(14.0)
            .font_weight(500)
            .text_color(sigil::prelude::Token::Gray12),
        )
        .child(
            sigil::kernel::text(session::short(&app.model.get().session).to_string())
                .nowrap()
                .text_color(sigil::prelude::Token::OnSurface)
                .font_size(12.0)
                .text_color(sigil::prelude::Token::Gray11),
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

fn notice_bar(app: App, band: chat::panels::Band) -> NodeBuilder {
    match band {
        chat::panels::Band::Approval { detail, persist } => {
            chat::panels::approval_band(app, usize::MAX, detail, persist)
        }
        chat::panels::Band::Form { lines, review } => {
            chat::form::form_band(app, usize::MAX, lines, review)
        }
        chat::panels::Band::Busy(status) => notice_band(status, Tone::Neutral),
        chat::panels::Band::Attached(count) => {
            notice_band(chat::panels::attached_note(&app, count), Tone::Info)
        }
    }
}
