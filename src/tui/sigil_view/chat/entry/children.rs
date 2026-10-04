use sigil::prelude::{AlignItems, LayoutBuilderExt, LayoutStyle, NodeBuilder, Token, container};

use super::super::{Agent, App};
use super::tools::{call_line, glyph, header, workspace_relative};
use super::tree::{branch, label};
use crate::tui::ToolState;
use crate::tui::sigil_view::look::Look;

fn state_of(kids: &[&Agent]) -> ToolState {
    if kids.iter().any(|kid| !kid.done) {
        ToolState::Running
    } else if kids.iter().any(|kid| kid.failed) {
        ToolState::Failed
    } else {
        ToolState::Ok
    }
}

fn totals(kids: &[&Agent]) -> String {
    let running = kids.iter().filter(|kid| !kid.done).count();
    let failed = kids.iter().filter(|kid| kid.failed).count();
    let tokens: u64 = kids.iter().map(|kid| kid.tokens).sum();
    let clock = kids
        .iter()
        .max_by_key(|kid| kid.elapsed())
        .map(|kid| kid.clock())
        .unwrap_or_default();
    let progress = match (running, failed) {
        (0, 0) => format!("{} done", kids.len()),
        (0, failed) => format!("{} done · {failed} failed", kids.len() - failed),
        (running, _) => format!("{running} running"),
    };
    format!(
        "{progress} · {clock} · {} tok",
        crate::provider::compact(tokens)
    )
}

fn child_row(kid: &Agent, first: bool, roots: &[String]) -> NodeBuilder {
    let state = kid.state();
    let (mark, tone) = glyph(&state);
    let (title, note) = match state {
        ToolState::Running => (
            Look::xs(Token::Gray12),
            format!(
                "{} · {} tok",
                kid.clock(),
                crate::provider::compact(kid.tokens)
            ),
        ),
        ToolState::Ok => (Look::xs(Token::Gray11), kid.last.clone()),
        ToolState::Failed => (Look::xs(Token::Gray12), kid.last.clone()),
    };
    let note_look = if matches!(state, ToolState::Failed) {
        Look::xs(Token::Error11)
    } else {
        Look::xs(Token::Gray10)
    };
    let line = container()
        .layout(
            LayoutStyle::flex_row()
                .grow(1.0)
                .gap(8.0)
                .min_size(0.0, 0.0)
                .align_items(AlignItems::Center),
        )
        .child(label(mark, tone))
        .child(label(kid.title.clone(), title))
        .child(label(workspace_relative(&note, roots), note_look).ellipsize());
    let mut rows = container()
        .layout(LayoutStyle::flex_column().fill_width().gap(0.0))
        .child(branch(first, line));
    if matches!(state, ToolState::Running) && !kid.last.is_empty() {
        rows = rows.child(branch(
            false,
            label(
                format!("  {}", workspace_relative(&kid.last, roots)),
                Look::xs(Token::Gray10),
            )
            .ellipsize(),
        ));
    }
    rows
}

pub(super) fn task_card(app: &App, call: &str) -> Option<NodeBuilder> {
    let model = app.model.get();
    let kids: Vec<&Agent> = model
        .agents
        .iter()
        .filter(|kid| !call.is_empty() && kid.call == call)
        .collect();
    if kids.is_empty() {
        return None;
    }
    if kids.iter().all(|kid| kid.background) {
        let what = if kids.len() == 1 {
            kids[0].title.clone()
        } else {
            crate::text::plural(kids.len(), "child")
        };
        let detail = "background".to_string();
        return Some(header(
            &ToolState::Running,
            call_line("Task", &what, Some((detail, Look::xs(Token::Gray10))), true),
        ));
    }
    let head = crate::text::plural(kids.len(), "child");
    let mut card = container()
        .layout(LayoutStyle::flex_column().fill_width().gap(0.0))
        .child(header(
            &state_of(&kids),
            call_line(
                "Task",
                &head,
                Some((totals(&kids), Look::xs(Token::Gray10))),
                true,
            ),
        ));
    for (at, kid) in kids.iter().enumerate() {
        card = card.child(child_row(kid, at == 0, &app.roots));
    }
    Some(card)
}
