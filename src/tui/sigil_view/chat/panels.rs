use sigil::prelude::{
    AlignItems, LayoutBuilderExt, LayoutStyle, NodeBuilder, Tone, button, card, container,
    icon_button, label, list_row, notice_band, section_label, spacer,
};

use super::composer;
use super::{App, Model, canvas};
use super::{busy_status, secret_row};

pub(super) fn agents(app: App, full_width: usize, width: usize) -> NodeBuilder {
    let update = app.clone();
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .shrink(0.0)
                .min_size(0.0, 0.0)
                .padding_x(16.0),
        )
        .children_reactive(move || {
            let _panes = update.panes.get();
            let _ticks = update.ticks.get();
            let model = update.model.borrow();
            let live = model.agents.iter().filter(|agent| !agent.done).count();
            if model.agents_hidden || live == 0 || width < 12 {
                return Vec::new();
            }
            vec![agent_panel(update.clone(), &model, full_width, width)]
        })
}

fn agent_panel(app: App, model: &Model, _full_width: usize, width: usize) -> NodeBuilder {
    let count = model.agents.iter().filter(|agent| !agent.done).count();
    let title = if count == 1 {
        "1 agent".to_string()
    } else {
        format!("{count} agents")
    };
    let close = app.clone();
    let mut body = container()
        .layout(LayoutStyle::flex_column().fill_width().gap(0.0))
        .child(
            container()
                .layout(
                    LayoutStyle::flex_row()
                        .fill_width()
                        .align_items(AlignItems::Center),
                )
                .child(section_label(title))
                .child(spacer())
                .child(icon_button("×", move || close.hide_agents()).accessible("Hide agents")),
        );
    for agent in model.agents.iter().filter(|agent| !agent.done).take(6) {
        let state = if agent.failed {
            "!"
        } else {
            composer::SPINNER[model.spinner % composer::SPINNER.len()]
        };
        body = body.child(list_row(
            false,
            format!("{state} {}", agent.id),
            canvas::ellipsize(&agent.last, width.saturating_sub(8)),
        ));
    }
    card(body)
}

pub(super) fn bands(app: App, _full_width: usize, width: usize) -> NodeBuilder {
    let update = app.clone();
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .shrink(0.0)
                .min_size(0.0, 0.0)
                .padding_x(16.0),
        )
        .children_reactive(move || {
            let _panes = update.panes.get();
            let _ticks = update.ticks.get();
            let approval = update.approval.get();
            let secret = update.secret.get();
            let model = update.model.borrow();
            let mut nodes = Vec::new();
            if let Some((detail, persist)) = approval {
                nodes.push(above(approval_band(update.clone(), width, detail, persist)));
            } else if let Some((prompt, typed)) = secret {
                nodes.push(above(notice_band(
                    secret_row(&prompt, typed, width),
                    Tone::Warn,
                )));
            } else if let Some(status) = busy_status(&model) {
                nodes.push(above(notice_band(status, Tone::Neutral)));
            } else if update.attached.get() > 0 {
                let attached = update.attached.peek();
                let noun = if attached == 1 { "image" } else { "images" };
                nodes.push(above(notice_band(
                    format!("{attached} {noun} attached · enter sends with your prompt"),
                    Tone::Info,
                )));
            }
            nodes
        })
}

fn above(child: NodeBuilder) -> NodeBuilder {
    container()
        .layout(LayoutStyle::flex_column().fill_width().padding_top(8.0))
        .child(child)
}

fn approval_band(app: App, width: usize, detail: String, persist: bool) -> NodeBuilder {
    let text = canvas::ellipsize(&detail, width.saturating_sub(4));
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
        .child(label("The agent is waiting. Choose Allow or Deny."))
        .child(actions)
}
