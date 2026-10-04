use sigil::prelude::Paint;
use sigil::prelude::{
    AlignItems, ControlSize, JustifyContent, LayoutBuilderExt, LayoutStyle, LogicalKey,
    ModifiersState, NodeBuilder, NodeEventsExt, Shortcut, Token, Tone, WidgetLayoutExt, button,
    container, icon_button, node, notice_band, spacer, text_edit,
};

use super::icons;
use super::{App, Pane, chat};
use crate::session;

mod composer;
pub(in crate::tui::sigil_view) mod status;

pub(super) const XS: f32 = 4.0;
pub(super) const S: f32 = 8.0;
pub(super) const M: f32 = 12.0;
pub(super) const L: f32 = 16.0;
pub(super) const XL: f32 = 24.0;
pub(super) const BODY: f32 = 13.0;
pub(super) const META: f32 = 12.0;
const COLUMN: f32 = 760.0;
const TITLE_CELLS: usize = 48;
const NEW_SESSION: &str = "New session";
const CRUMB: &str = "›";
const EXPLAIN: &str = "Explain this repo";

pub(super) fn quiet(content: impl Into<sigil::kernel::TextContent>) -> sigil::kernel::TextBuilder {
    sigil::kernel::text(content)
        .nowrap()
        .font_size(META)
        .text_color(Token::OnSurfaceMuted)
}

pub(super) fn strong(content: impl Into<sigil::kernel::TextContent>) -> sigil::kernel::TextBuilder {
    sigil::kernel::text(content)
        .nowrap()
        .font_size(BODY)
        .font_weight(600)
        .text_color(Token::OnSurface)
}

pub(super) fn model_label(app: &App) -> String {
    let model = app.model.get();
    match model.snapshot.effort {
        Some(effort) => format!("{} · {}", model.snapshot.model, effort.as_str()),
        None => model.snapshot.model.clone(),
    }
}

fn column(child: NodeBuilder) -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .shrink(0.0)
                .padding_x(XL)
                .justify_content(JustifyContent::Center),
        )
        .child(
            container()
                .layout(
                    LayoutStyle::flex_column()
                        .fill_width()
                        .max_width(COLUMN)
                        .align_items(AlignItems::Stretch),
                )
                .child(child),
        )
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
                .align_items(AlignItems::Stretch),
        )
        .bg(Token::Surface)
        .child(node(move || {
            chat::transcript(transcript_app, (XL + L, L), empty_home)
        }))
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
}

fn active(app: &App) -> Option<crate::tui::hub::Live> {
    app.model
        .get()
        .live
        .iter()
        .find(|entry| entry.active)
        .cloned()
}

fn workspace_name(app: &App) -> String {
    active(app)
        .map_or_else(|| app.workspace.clone(), |entry| entry.workspace)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "Summonr".to_string())
}

fn session_title(app: &App) -> String {
    active(app)
        .map(|entry| crate::text::ellipsize(entry.title.trim(), TITLE_CELLS))
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| NEW_SESSION.to_string())
}

pub(super) fn title_bar(app: App) -> NodeBuilder {
    let place = {
        let app = app.clone();
        sigil::reactive::derived(move || (workspace_name(&app), session_title(&app)))
    };
    let changes = {
        let model = app.model;
        sigil::reactive::derived(move || {
            let model = model.get();
            (
                model.diff.files.len(),
                model.diff.added(),
                model.diff.removed(),
            )
        })
    };
    let chip_label = {
        let label = app.clone();
        sigil::reactive::derived(move || model_label(&label))
    };
    let mut crumbs = container()
        .layout(
            LayoutStyle::flex_row()
                .gap(S)
                .min_size(0.0, 0.0)
                .align_items(AlignItems::Center),
        )
        .child(
            container()
                .layout(LayoutStyle::block().shrink(0.0))
                .children_reactive(move || vec![quiet(place.get().0)]),
        );
    if let Some(branch) = app.git_branch.clone().filter(|branch| !branch.is_empty()) {
        crumbs = crumbs.child(quiet(CRUMB)).child(quiet(branch));
    }
    crumbs = crumbs.child(quiet(CRUMB)).child(
        container()
            .layout(LayoutStyle::block().min_size(0.0, 0.0))
            .children_reactive(move || vec![strong(place.get().1)]),
    );
    let picker = app.clone();
    let options = app.clone();
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .height(46.0)
                .padding_x(L)
                .gap(M)
                .align_items(AlignItems::Center),
        )
        .child(crumbs)
        .child(spacer())
        .child(
            button(move || chip_label.get())
                .ghost()
                .size(ControlSize::Compact)
                .accessible("Model")
                .activate(move || picker.begin_model_edit()),
        )
        .child(
            container()
                .layout(LayoutStyle::flex_row().align_items(AlignItems::Center))
                .children_reactive({
                    let review = app.clone();
                    move || {
                        let (files, added, removed) = changes.get();
                        if files == 0 {
                            return Vec::new();
                        }
                        let open = review.clone();
                        vec![
                            button(format!("Review +{added} −{removed}"))
                                .size(ControlSize::Compact)
                                .activate(move || open.toggle_pane(Pane::Diff)),
                        ]
                    }
                }),
        )
        .child(icon_button("···", move || options.toggle_options()).accessible("Options"))
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
    if !app.model.peek().diff.files.is_empty() {
        let review = app.clone();
        actions = actions.child(
            button("Review uncommitted changes")
                .size(ControlSize::Compact)
                .activate(move || review.toggle_pane(Pane::Diff)),
        );
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
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .padding_y(XL)
                .gap(L)
                .align_items(AlignItems::Center),
        )
        .child(
            sigil::kernel::text(workspace_name(app))
                .nowrap()
                .font_size(20.0)
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
        chat::panels::Band::Busy(status) => notice_band(status, Tone::Neutral),
        chat::panels::Band::Attached(count) => {
            notice_band(chat::panels::attached_note(&app, count), Tone::Info)
        }
    }
}
