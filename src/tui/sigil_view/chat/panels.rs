use sigil::prelude::{
    AlignItems, LayoutBuilderExt, LayoutStyle, ListRowSlots, MouseButton, NodeBuilder,
    NodeEventsExt, NodeStyleExt, Tone, button, card, container, icon_button, list_row_with,
    notice_band, section_label, spacer, spinner_frame,
};

use super::busy_status;
use super::{Agent, App, Model};

#[derive(Clone, PartialEq)]
struct AgentView {
    expanded: bool,
    headline: String,
    detail: String,
    spend: String,
    rows: Vec<AgentRow>,
}

#[derive(Clone, PartialEq)]
struct AgentRow {
    label: String,
    detail: String,
    metrics: String,
    last: String,
}

fn agent_view(app: &App, model: &Model, width: usize) -> AgentView {
    let running = model.agents.iter().filter(|agent| !agent.done).count();
    let tick = if running > 0 { app.ticks.get() } else { 0 };
    let tokens: u64 = model.agents.iter().map(|agent| agent.tokens).sum();
    let cost: f64 = model.agents.iter().map(|agent| agent.cost).sum();
    let priced = model.agents.iter().any(|agent| agent.cost > 0.0);
    let total = model.agents.len();
    let noun = if total == 1 { "child" } else { "children" };
    let glyph = if running == 0 {
        "✓".to_string()
    } else {
        spinner_frame(tick).to_string()
    };
    AgentView {
        expanded: model.agents_expanded,
        headline: format!("{glyph} {total} {noun}"),
        detail: format!("{running} running"),
        spend: {
            let spend = spend(tokens, cost, priced);
            if model.agents_expanded {
                format!("{running} running · {spend}")
            } else {
                format!("{spend} ▸")
            }
        },
        rows: if model.agents_expanded {
            model
                .agents
                .iter()
                .map(|agent| AgentRow {
                    label: format!("{} {}", agent_glyph(agent, tick), agent.id),
                    detail: crate::text::ellipsize(&agent.assignment, width.saturating_sub(44))
                        .to_string(),
                    metrics: {
                        let access = match agent.access {
                            crate::child::Access::Implement => "rw",
                            crate::child::Access::Research => "ro",
                        };
                        let cost = if priced && agent.cost > 0.0 {
                            format!(" · {}", crate::dollars(agent.cost))
                        } else {
                            String::new()
                        };
                        format!(
                            "{access} · {} · {}{cost}",
                            clock(agent.elapsed()),
                            crate::provider::compact(agent.tokens),
                        )
                    },
                    last: crate::text::ellipsize(&agent.last, width.saturating_sub(6)).to_string(),
                })
                .collect()
        } else {
            Vec::new()
        },
    }
}

pub(in crate::tui::sigil_view) fn agents(app: App, width: usize) -> NodeBuilder {
    let update = app.clone();
    let view = sigil::reactive::derived(move || {
        let model = update.model.get();
        if model.agents_hidden || model.agents.is_empty() || width < 12 {
            None
        } else {
            Some(agent_view(&update, &model, width))
        }
    });
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .shrink(0.0)
                .min_size(0.0, 0.0),
        )
        .children_reactive(move || {
            view.get()
                .map(|view| vec![agent_panel(app.clone(), view)])
                .unwrap_or_default()
        })
}

fn agent_glyph(agent: &Agent, tick: u64) -> String {
    if agent.done {
        if agent.failed { "✗" } else { "✓" }.to_string()
    } else {
        spinner_frame(tick).to_string()
    }
}

fn clock(secs: u64) -> String {
    if secs >= 60 {
        format!("{}:{:02}", secs / 60, secs % 60)
    } else {
        format!("{secs}s")
    }
}

fn spend(tokens: u64, cost: f64, priced: bool) -> String {
    let cost = priced.then(|| format!(" · {}", crate::dollars(cost)));
    format!(
        "{} tok{}",
        crate::provider::compact(tokens),
        cost.unwrap_or_default()
    )
}

fn agent_panel(app: App, view: AgentView) -> NodeBuilder {
    if !view.expanded {
        let toggle = app.clone();
        return card(
            list_row_with(
                false,
                view.headline,
                ListRowSlots {
                    detail: Some(view.detail.into()),
                    status: Some(view.spend.into()),
                    shrink: true,
                    ..ListRowSlots::default()
                },
            )
            .pointer_down(move |event| {
                if event.button == Some(MouseButton::Left) {
                    toggle.toggle_agents();
                    event.stop_propagation();
                }
            }),
        );
    }
    let close = app.clone();
    let mut body = container()
        .layout(LayoutStyle::flex_column().fill_width().gap(0.0))
        .child(
            container()
                .layout(
                    LayoutStyle::flex_row()
                        .fill_width()
                        .gap(4.0)
                        .align_items(AlignItems::Center),
                )
                .child(section_label(view.spend.clone()))
                .child(spacer())
                .child(
                    icon_button("▾", move || close.toggle_agents()).accessible("Collapse agents"),
                )
                .child(
                    icon_button("✕", {
                        let app = app.clone();
                        move || app.hide_agents()
                    })
                    .accessible("Hide agents"),
                ),
        );
    for row in view.rows {
        let slots = ListRowSlots {
            detail: (!row.detail.is_empty()).then(|| row.detail.into()),
            status: Some(row.metrics.into()),
            shrink: true,
            ..ListRowSlots::default()
        };
        body = body.child(list_row_with(false, row.label, slots));
        if !row.last.is_empty() {
            body = body.child(
                sigil::kernel::text(row.last)
                    .nowrap()
                    .class(sigil::style::Token::OnSurface.text())
                    .class("text-xs text-gray-10")
                    .layout(LayoutStyle::block().padding_x(12.0)),
            );
        }
    }
    card(body)
}

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
                .class(sigil::style::Token::OnSurface.text())
                .class("text-xs text-accent"),
        )
        .child(
            sigil::kernel::text(rest.to_owned())
                .nowrap()
                .class(sigil::style::Token::OnSurface.text())
                .class("text-xs text-gray-11"),
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
                .class(sigil::style::Token::OnSurface.text()),
        )
        .child(actions)
}
