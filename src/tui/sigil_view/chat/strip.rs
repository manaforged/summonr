use sigil::prelude::{
    AlignItems, LayoutBuilderExt, LayoutStyle, MouseButton, NodeBuilder, NodeEventsExt, Token,
    container,
};

use super::{Agent, App};
use crate::tui::ToolState;
use crate::tui::sigil_view::look::Look;
use crate::tui::sigil_view::ui::{Hints, Key};

fn text(body: impl Into<String>, look: Look) -> sigil::kernel::TextBuilder {
    look.on(sigil::kernel::text(body.into()).nowrap())
}

fn mark(agent: &Agent) -> (&'static str, Look) {
    match agent.state() {
        ToolState::Running => ("○", Look::xs(Token::Gray11)),
        ToolState::Ok => ("●", Look::xs(Token::Success11)),
        ToolState::Failed => ("●", Look::xs(Token::Error11)),
    }
}

fn line(agent: &Agent, lead: &str, roots: &[String], detail: bool) -> NodeBuilder {
    let (glyph, tone) = mark(agent);
    let meta = if detail {
        let access = match agent.access {
            crate::child::Access::Implement => "rw",
            crate::child::Access::Research => "ro",
        };
        format!(
            "{} · {access} · {} · {} tok",
            agent.id,
            agent.clock(),
            crate::provider::compact(agent.tokens)
        )
    } else {
        format!(
            "{} · {} tok",
            agent.clock(),
            crate::provider::compact(agent.tokens)
        )
    };
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .gap(8.0)
                .min_size(0.0, 0.0)
                .align_items(AlignItems::Center),
        )
        .child(text(lead.to_string(), Look::xs(Token::Gray10)))
        .child(text(glyph, tone))
        .child(text(agent.title.clone(), Look::xs(Token::Gray12)))
        .child(
            text(
                super::entry::workspace_relative(&agent.last, roots),
                Look::xs(Token::Gray10),
            )
            .ellipsize()
            .layout(LayoutStyle::block().grow(1.0).min_size(0.0, 0.0)),
        )
        .child(text(meta, Look::xs(Token::Gray10)))
}

fn shown(app: &App) -> (bool, Vec<Agent>) {
    let model = app.model.get();
    if model.agents_hidden {
        return (false, Vec::new());
    }
    let expanded = model.agents_expanded;
    let rows = model
        .agents
        .iter()
        .filter(|agent| expanded || (agent.background && !agent.done))
        .cloned()
        .collect();
    (expanded, rows)
}

pub(in crate::tui::sigil_view) fn rows(app: &App) -> usize {
    let (expanded, rows) = shown(app);
    rows.len() + usize::from(expanded && !rows.is_empty())
}

pub(in crate::tui::sigil_view) fn agents(app: App, width: usize) -> NodeBuilder {
    let update = app.clone();
    let view = sigil::reactive::derived(move || {
        update.ticks.with(|_| ());
        if width >= 12 {
            shown(&update)
        } else {
            (false, Vec::new())
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
            let (expanded, rows) = view.get();
            let mut nodes = Vec::new();
            if expanded && !rows.is_empty() {
                let tokens: u64 = rows.iter().map(|agent| agent.tokens).sum();
                let toggle = app.clone();
                let close = Hints::new()
                    .key(Key::Hotkey(crate::prefs::Hotkey::Agents), "closes")
                    .text(&app.model.peek().prefs.hotkeys);
                nodes.push(
                    container()
                        .layout(LayoutStyle::flex_row().fill_width().gap(8.0))
                        .child(text("children", Look::xs(Token::Gray12).bold()))
                        .child(text(
                            format!(
                                "{} · {} tok · {close}",
                                rows.len(),
                                crate::provider::compact(tokens)
                            ),
                            Look::xs(Token::Gray10),
                        ))
                        .pointer_down(move |event| {
                            if event.button == Some(MouseButton::Left) {
                                toggle.toggle_agents();
                                event.stop_propagation();
                            }
                        }),
                );
            }
            for agent in &rows {
                let lead = format!("{:<10}", if agent.background { "background" } else { "" });
                nodes.push(line(agent, &lead, &app.roots, expanded));
            }
            nodes
        })
}
