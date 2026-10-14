use sigil::prelude::{AlignItems, LayoutBuilderExt, LayoutStyle, NodeBuilder, Token, container};

use super::super::{Agent, App, Call};
use super::preview;
use super::tools::{call_line, header, workspace_relative};
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

fn subagent_line(kid: &Agent, roots: &[String]) -> NodeBuilder {
    let title = task_title(kid, roots);
    let (tone, status, doing) = match kid.state() {
        ToolState::Running => {
            let last = workspace_relative(&kid.last, roots);
            let doing = (!last.is_empty()).then(|| format!("· {last}"));
            (Token::Accent11, format!("running: “{title}”"), doing)
        }
        ToolState::Ok => (
            Token::Success11,
            format!("completed in {}: “{title}”", kid.clock()),
            None,
        ),
        ToolState::Failed => (
            Token::Error11,
            format!("failed after {}: “{title}”", kid.clock()),
            None,
        ),
    };
    let mut line = container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .gap(8.0)
                .min_size(0.0, 0.0)
                .align_items(AlignItems::Center),
        )
        .child(label("◆", Look::xs(tone)))
        .child(label("Subagent", Look::xs(Token::Gray12).bold()));
    if !kid.agent.is_empty() {
        line = line.child(label(kid.agent.clone(), Look::xs(Token::Info11)));
    }
    line = line.child(
        label(status, Look::xs(Token::Gray10))
            .ellipsize()
            .layout(LayoutStyle::block().shrink(4.0).min_size(0.0, 0.0)),
    );
    match doing {
        Some(doing) => line.child(
            label(doing, Look::xs(Token::Gray10))
                .ellipsize()
                .layout(LayoutStyle::block().shrink(1.0).min_size(0.0, 0.0)),
        ),
        None => line,
    }
}

pub(super) fn starting_card() -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .gap(8.0)
                .min_size(0.0, 0.0)
                .align_items(AlignItems::Center),
        )
        .child(label("◆", Look::xs(Token::Accent11)))
        .child(label("Subagent", Look::xs(Token::Gray12).bold()))
        .child(label("starting…", Look::xs(Token::Gray10)))
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
    let receipt = model.child_receipt(call);
    let kept = receipt.as_ref().and_then(|record| {
        let tag = crate::child::tag(&record.id.to_string());
        kept_of(record).map(|kept| (tag, kept))
    });
    let mut card = container().layout(LayoutStyle::flex_column().fill_width().gap(0.0));
    for kid in kids {
        card = card.child(subagent_line(kid, &kid.roots(&app.roots)));
        if let Some((_, kept)) = kept.as_ref().filter(|(tag, _)| *tag == kid.id) {
            card = card.child(branch(
                true,
                label(kept_line(kept, &app.roots), Look::xs(Token::Warn11)).ellipsize(),
            ));
        }
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
