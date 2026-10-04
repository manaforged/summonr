use sigil::prelude::Paint;
use sigil::prelude::{
    AlignItems, LayoutBuilderExt, LayoutStyle, NodeBuilder, Tone, button, container, notice_band,
};

use super::App;
use super::busy_status;

pub(in crate::tui::sigil_view) use super::strip::{agents, rows as strip_rows};

#[derive(Clone, PartialEq)]
pub(in crate::tui::sigil_view) enum Band {
    Approval { detail: String, persist: bool },
    Form { lines: Vec<String>, review: bool },
    Busy(String),
    Attached(usize),
}

pub(in crate::tui::sigil_view) fn band(app: &App) -> Option<Band> {
    if let Some((detail, persist)) = app.approval.get() {
        return Some(Band::Approval { detail, persist });
    }
    if let Some(state) = app.form.get() {
        return Some(Band::Form {
            lines: state.lines(),
            review: state.stage == crate::tui::sigil_view::app::Stage::Review,
        });
    }
    let attached = app.attached.get();
    if let Some(status) = busy_status(&app.model.get(), app.ticks.get()) {
        return Some(Band::Busy(if attached > 0 {
            format!("{status} · {}", attached_text(attached))
        } else {
            status
        }));
    }
    (attached > 0).then_some(Band::Attached(attached))
}

pub(in crate::tui::sigil_view) fn attached_text(attached: usize) -> String {
    let noun = if attached == 1 { "image" } else { "images" };
    format!("{attached} {noun} attached")
}

pub(super) fn band_nodes(app: &App, width: usize, band: Band) -> NodeBuilder {
    match band {
        Band::Approval { detail, persist } => {
            above(approval_band(app.clone(), width, detail, persist))
        }
        Band::Form { lines, review } => {
            above(super::form::form_band(app.clone(), width, lines, review))
        }
        Band::Busy(status) => busy_row(&crate::text::ellipsize(&status, width)),
        Band::Attached(attached) => above(notice_band(
            format!("{} · enter sends with your prompt", attached_text(attached)),
            Tone::Info,
        )),
    }
}

pub(super) fn bands(app: App, width: usize) -> NodeBuilder {
    let update = app.clone();
    let active = sigil::reactive::derived(move || band(&update));
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .shrink(0.0)
                .min_size(0.0, 0.0),
        )
        .children_reactive(move || {
            active
                .get()
                .map(|band| vec![band_nodes(&app, width, band)])
                .unwrap_or_default()
        })
}

fn busy_row(status: &str) -> NodeBuilder {
    let (spinner, rest) = status.split_at(status.find(' ').unwrap_or(status.len()));
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .shrink(0.0)
                .min_size(0.0, 0.0),
        )
        .child(
            sigil::kernel::text(spinner.to_owned())
                .nowrap()
                .text_color(sigil::prelude::Token::OnSurface)
                .font_size(12.0)
                .text_color(sigil::prelude::Token::Accent),
        )
        .child(
            sigil::kernel::text(rest.to_owned())
                .nowrap()
                .text_color(sigil::prelude::Token::OnSurface)
                .font_size(12.0)
                .text_color(sigil::prelude::Token::Gray11),
        )
}

fn above(child: NodeBuilder) -> NodeBuilder {
    container()
        .layout(LayoutStyle::flex_column().fill_width().padding_top(8.0))
        .child(child)
}

fn approval_band(app: App, width: usize, detail: String, persist: bool) -> NodeBuilder {
    let text = crate::text::ellipsize(&detail, width.saturating_sub(4));
    let mut actions = container().layout(
        LayoutStyle::flex_row()
            .gap(12.0)
            .align_items(AlignItems::Center),
    );
    actions = actions.child(button("Allow").primary().activate({
        let app = app.clone();
        move || app.approval_answer("y")
    }));
    if persist {
        actions = actions.child(button("Always").activate({
            let app = app.clone();
            move || app.approval_answer("always")
        }));
    }
    actions = actions.child(
        button("Deny")
            .destructive()
            .activate(move || app.approval_answer("n")),
    );
    notice_band(format!("Needs approval: {text}"), Tone::Warn)
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .gap(8.0)
                .padding_y(8.0),
        )
        .child(
            sigil::kernel::text("The agent is waiting. Choose Allow or Deny.")
                .nowrap()
                .text_color(sigil::prelude::Token::OnSurface),
        )
        .child(actions)
}
