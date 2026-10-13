use sigil::prelude::{AlignItems, LayoutBuilderExt, LayoutStyle, NodeBuilder, Token, container};

use super::super::{Agent, App, Call};
use super::preview;
use super::tools::{call_line, glyph, header, workspace_relative};
use super::tree::{branch, label, tree};
use crate::child::{ChildRecord, ChildState};
use crate::tools::{Hold, Kept};
use crate::tui::ToolState;
use crate::tui::sigil_view::look::Look;

fn task_title(kid: &Agent, roots: &[String]) -> String {
    if kid.assignment.is_empty() {
        return workspace_relative(&kid.title, roots);
    }
    Agent::title_of(&workspace_relative(&kid.assignment, roots))
}

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

fn child_row(kid: &Agent, first: bool, parents: &[String], kept: Option<&Kept>) -> NodeBuilder {
    let roots = &kid.roots(parents);
    let state = kid.state();
    let (mark, tone) = match kept {
        Some(_) => ("●", Look::xs(Token::Warn11)),
        None => glyph(&state),
    };
    let title = match state {
        ToolState::Ok => Look::xs(Token::Gray11),
        ToolState::Running | ToolState::Failed => Look::xs(Token::Gray12),
    };
    let mut line = container()
        .layout(
            LayoutStyle::flex_row()
                .grow(1.0)
                .gap(8.0)
                .min_size(0.0, 0.0)
                .align_items(AlignItems::Center),
        )
        .child(label(mark, tone))
        .child(
            label(task_title(kid, roots), title)
                .ellipsize()
                .layout(LayoutStyle::block().shrink(1.0).min_size(0.0, 0.0)),
        );
    if !kid.agent.is_empty() {
        line = line.child(label(kid.agent.clone(), Look::xs(Token::Info11)));
    }
    line = line.child(label(kid.access.name(), Look::xs(Token::Gray10)));
    if matches!(state, ToolState::Running) {
        line = line.child(label(
            format!(
                "{} · {} tok",
                kid.clock(),
                crate::provider::compact(kid.tokens)
            ),
            Look::xs(Token::Gray10),
        ));
    }
    let mut rows = container()
        .layout(LayoutStyle::flex_column().fill_width().gap(0.0))
        .child(branch(first, line));
    if let Some(kept) = kept {
        rows = rows.child(branch(
            false,
            label(
                format!("  {}", kept_line(kept, parents)),
                Look::xs(Token::Warn11),
            )
            .ellipsize(),
        ));
    }
    if !kid.last.is_empty() {
        let look = if matches!(state, ToolState::Failed) {
            Look::xs(Token::Error11)
        } else {
            Look::xs(Token::Gray10)
        };
        rows = rows.child(branch(
            false,
            label(format!("  {}", workspace_relative(&kid.last, roots)), look).ellipsize(),
        ));
    }
    rows
}

fn kept_line(kept: &Kept, roots: &[String]) -> String {
    let checkout = roots
        .iter()
        .filter_map(|root| std::path::Path::new(root).parent())
        .find_map(|parent| kept.checkout.strip_prefix(parent).ok())
        .unwrap_or(&kept.checkout);
    format!(
        "kept {}: {} · remove with `rm -rf {}` when done",
        checkout.display(),
        kept.hold.reason(),
        checkout.display()
    )
}

fn kept_of(record: &ChildRecord) -> Option<&Kept> {
    match &record.state {
        ChildState::Running => None,
        ChildState::Completed(result)
        | ChildState::Aborted(result)
        | ChildState::Failed(result)
        | ChildState::Interrupted(result) => result.kept.as_ref(),
    }
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
            task_title(kids[0], &kids[0].roots(&app.roots))
        } else {
            crate::text::plural(kids.len(), "child")
        };
        let detail = "subagent".to_string();
        return Some(header(
            &state_of(&kids),
            call_line("Task", &what, Some((detail, Look::xs(Token::Gray10))), true),
        ));
    }
    let receipt = model.child_receipt(call);
    let kept = receipt.as_ref().and_then(|record| {
        let tag = crate::child::tag(&record.id.to_string());
        kept_of(record).map(|kept| (tag, kept))
    });
    let head = match kids.as_slice() {
        [kid] => task_title(kid, &kid.roots(&app.roots)),
        [] | [_, _, ..] => crate::text::plural(kids.len(), "child"),
    };
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
        let held = kept
            .as_ref()
            .filter(|(tag, _)| *tag == kid.id)
            .map(|(_, kept)| *kept);
        card = card.child(child_row(kid, at == 0, &app.roots, held));
    }
    Some(card)
}

fn receipt_state(state: &ChildState) -> (ToolState, &'static str) {
    match state {
        ChildState::Running => (ToolState::Running, "running"),
        ChildState::Completed(result) => match result.kept.as_ref().map(|kept| kept.hold) {
            Some(Hold::Conflict) => (ToolState::Ok, "conflict"),
            Some(Hold::OffBase | Hold::Unreadable | Hold::Unremoved) => (ToolState::Ok, "kept"),
            None => (ToolState::Ok, "completed"),
        },
        ChildState::Aborted(_) => (ToolState::Failed, "aborted"),
        ChildState::Failed(_) => (ToolState::Failed, "failed"),
        ChildState::Interrupted(_) => (ToolState::Failed, "interrupted"),
    }
}

pub(super) fn receipt_card(
    call: &Call,
    full: bool,
    hidden: bool,
    app: &App,
) -> Option<NodeBuilder> {
    let record: ChildRecord = serde_json::from_str(&call.output).ok()?;
    let (state, word) = receipt_state(&record.state);
    let mut roots = vec![record.workspace.to_string_lossy().into_owned()];
    roots.extend(app.roots.iter().cloned());
    let note = format!("{} · {word}", record.access.name());
    let card = container()
        .layout(LayoutStyle::flex_column().fill_width().gap(0.0))
        .child(header(
            &state,
            call_line(
                "Task",
                &Agent::title_of(&workspace_relative(&record.assignment, &roots)),
                Some((note, Look::xs(Token::Gray10))),
                true,
            ),
        ));
    let result = match &record.state {
        ChildState::Running => return Some(card),
        ChildState::Completed(result)
        | ChildState::Aborted(result)
        | ChildState::Failed(result)
        | ChildState::Interrupted(result) => result,
    };
    if hidden {
        return Some(card);
    }
    let mut body = preview::plain(&result.text);
    if let Some(kept) = &result.kept {
        body = format!("{body}\n{}", kept_line(kept, &roots[1..]))
            .trim()
            .to_string();
    } else if let Some(diagnostic) = &result.diagnostic {
        body = format!("{body}\n{diagnostic}").trim().to_string();
    }
    let failed = matches!(state, ToolState::Failed);
    let mut shown = preview::text(full, "Task", &body, failed, false);
    if !result.changed_paths.is_empty() {
        let paths = result
            .changed_paths
            .iter()
            .map(|path| workspace_relative(path, &roots))
            .collect::<Vec<_>>()
            .join(", ");
        let more = result
            .changed_paths_total
            .saturating_sub(result.changed_paths.len());
        let tail = if more == 0 {
            String::new()
        } else {
            format!(" +{more}")
        };
        shown = format!("{shown}\nchanged {paths}{tail}").trim().to_string();
    }
    Some(if shown.is_empty() {
        card
    } else {
        card.child(tree(&shown))
    })
}
