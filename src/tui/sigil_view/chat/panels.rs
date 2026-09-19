use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, MouseButton, NodeBuilder, NodeEventsExt, NodeStyleExt, Style,
    container, label,
};
use sigil_tui::cell_width;

use super::super::app::App;
use super::super::canvas::{self, Span, palette};
use super::super::model::Model;
use super::composer;
use super::surface::line;
use super::{accent, busy_status, margin, render_background, run, secret_row};

pub(super) fn agents(app: App, full_width: usize, width: usize) -> NodeBuilder {
    let update = app.clone();
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .shrink(0.0)
                .min_size(0.0, 0.0),
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

fn agent_panel(app: App, model: &Model, full_width: usize, width: usize) -> NodeBuilder {
    let count = model.agents.iter().filter(|agent| !agent.done).count();
    let title = if count == 1 {
        " 1 agent ".to_string()
    } else {
        format!(" {count} agents ")
    };
    let close = app.clone();
    let bg = palette().tool_bg;
    let title_row = container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .height(canvas::cell().h),
        )
        .style(Style::new().bg(bg))
        .child(
            render_background(
                [Span::surface(
                    format!(
                        "╭{title}{}",
                        "─".repeat(width.saturating_sub(cell_width(&title) + 5))
                    ),
                    palette().gray,
                    bg,
                )]
                .iter()
                .map(run),
                bg,
            )
            .layout(LayoutStyle::flex_row().grow(1.0).height(canvas::cell().h)),
        )
        .child(
            render_background(
                [Span::surface(" x ╮", palette().dim, bg)].iter().map(run),
                bg,
            )
            .layout(
                LayoutStyle::flex_row()
                    .width(4.0 * canvas::cell().w)
                    .height(canvas::cell().h),
            )
            .pointer_down(move |event| {
                if event.button == Some(MouseButton::Left) {
                    close.hide_agents();
                    event.stop_propagation();
                }
            }),
        );
    let mut panel = container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .min_size(0.0, 0.0)
                .padding_x(margin(full_width) as f32 * canvas::cell().w),
        )
        .child(title_row);
    for agent in model.agents.iter().filter(|agent| !agent.done).take(6) {
        let state = if agent.failed {
            "!"
        } else {
            composer::SPINNER[model.spinner % composer::SPINNER.len()]
        };
        let text = canvas::ellipsize(
            &format!(" {state} {} · {}", agent.id, agent.last),
            width.saturating_sub(2),
        );
        let text = format!(
            "│{text}{}│",
            " ".repeat(width.saturating_sub(cell_width(&text) + 2))
        );
        panel = panel.child(render_background(
            [Span::surface(text, palette().text, bg)].iter().map(run),
            bg,
        ));
    }
    panel.child(render_background(
        [Span::surface(
            format!("╰{}╯", "─".repeat(width.saturating_sub(2))),
            palette().gray,
            bg,
        )]
        .iter()
        .map(run),
        bg,
    ))
}

pub(super) fn bands(app: App, full_width: usize, width: usize) -> NodeBuilder {
    let update = app.clone();
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .shrink(0.0)
                .min_size(0.0, 0.0),
        )
        .children_reactive(move || {
            let _panes = update.panes.get();
            let _ticks = update.ticks.get();
            let approval = update.approval.get();
            let secret = update.secret.get();
            let model = update.model.borrow();
            let mut nodes = Vec::new();
            if let Some((detail, persist)) = approval {
                nodes.push(approval_band(
                    update.clone(),
                    full_width,
                    width,
                    detail,
                    persist,
                ));
            } else if let Some((prompt, typed)) = secret {
                nodes.push(line(
                    update.clone(),
                    vec![Span::styled(
                        secret_row(&prompt, typed, width),
                        palette().yellow,
                    )],
                    palette().base,
                    full_width,
                ));
            } else if let Some(status) = busy_status(&model) {
                nodes.push(line(
                    update.clone(),
                    composer::busy_line(&status, width, accent(&model.prefs.theme)),
                    palette().base,
                    full_width,
                ));
            } else if update.attached.get() > 0 {
                nodes.push(line(
                    update.clone(),
                    composer::attachment_line(update.attached.peek()),
                    palette().base,
                    full_width,
                ));
            }
            nodes
        })
}

fn approval_band(
    app: App,
    full_width: usize,
    width: usize,
    detail: String,
    persist: bool,
) -> NodeBuilder {
    let allow = "[ allow ]";
    let always = "[ always ]";
    let deny = "[ deny ]";
    let buttons =
        cell_width(allow) + cell_width(deny) + if persist { cell_width(always) + 1 } else { 0 };
    let text = canvas::ellipsize(
        &format!("approve {}", detail),
        width.saturating_sub(buttons + 3),
    );
    let mut row = container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .height(canvas::cell().h)
                .padding_x(margin(full_width) as f32 * canvas::cell().w),
        )
        .style(Style::new().bg(palette().base))
        .child(label(text).class("font-mono text-sm"));
    row = row.child(approval_button(app.clone(), allow, "y"));
    if persist {
        row = row.child(approval_button(app.clone(), always, "always"));
    }
    row.child(approval_button(app, deny, "n"))
}

fn approval_button(app: App, text: &'static str, token: &'static str) -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_row()
                .width(cell_width(text) as f32 * canvas::cell().w)
                .height(canvas::cell().h),
        )
        .child(label(text).class("font-mono text-sm"))
        .pointer_down(move |event| {
            if event.button == Some(MouseButton::Left) {
                app.approval_answer(token);
                event.stop_propagation();
            }
        })
}
