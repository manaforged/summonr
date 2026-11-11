use sigil::prelude::{NodeBuilder, Signal, Space, Token};

use super::super::{Agent, App, Call};
use super::preview;
use super::tools::{call_line, head_row, header, workspace_relative};
use super::tree::{branch, label, tree};
use crate::text_style::TextStyle;
use crate::{Outcome, ToolState};
use sigil::prelude::Widget;
use summonr_agent::child::{ChildRecord, ChildState};
use summonr_agent::tools::{Builtin, Kept};

fn subagent_row(mark: impl sigil::prelude::IntoNode + 'static) -> NodeBuilder {
    head_row(
        mark,
        label("Subagent", TextStyle::xs(Token::Accent11).bold()),
    )
}

fn task_title(kid: &Agent, roots: &[String]) -> String {
    if kid.assignment.is_empty() {
        return workspace_relative(&kid.title, roots);
    }
    Agent::title_of(&workspace_relative(&kid.assignment, roots))
}

fn subagent_line(kid: &Agent, roots: &[String], ticks: Signal<u64>) -> NodeBuilder {
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
    let mut line = if matches!(kid.state(), ToolState::Running) {
        subagent_row(super::tools::loading_mark(ticks))
    } else {
        subagent_row(label("◆", TextStyle::xs(tone)))
    };
    if !kid.agent.is_empty() {
        line = line.child(label(kid.agent.clone(), TextStyle::xs(Token::Info11)));
    }
    line = line.child(
        label(status, TextStyle::xs(Token::Gray10))
            .ellipsize()
            .shrink(4.0)
            .min_width(0.0)
            .min_height(0.0),
    );
    match doing {
        Some(doing) => line.child(
            label(doing, TextStyle::xs(Token::Gray10))
                .ellipsize()
                .shrink(1.0)
                .min_width(0.0)
                .min_height(0.0)
                .max_width(super::super::ACTIVITY_SHARE),
        ),
        None => line,
    }
}

pub(super) fn starting_card(ticks: Signal<u64>) -> NodeBuilder {
    subagent_row(super::tools::loading_mark(ticks))
        .child(label("starting…", TextStyle::xs(Token::Gray10)))
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

pub(super) fn task_card(app: &App, call: &str) -> Option<NodeBuilder> {
    let model = app.model.get();
    let kids: Vec<&Agent> = model.agents_of(call).collect();
    if kids.is_empty() {
        return None;
    }
    let receipt = model.child_receipt(call);
    let kept = receipt.as_ref().and_then(|record| {
        let tag = summonr_agent::child::tag(&record.id.to_string());
        record
            .state
            .result()
            .and_then(|result| result.kept.as_ref())
            .map(|kept| (tag, kept))
    });
    let mut card = sigil::prelude::column().fill_width().gap(Space::None);
    for kid in kids {
        card = card.child(subagent_line(kid, &kid.roots(&app.roots()), app.ticks));
        if let Some((_, kept)) = kept.as_ref().filter(|(tag, _)| *tag == kid.id) {
            card = card.child(branch(
                true,
                label(
                    kept_line(kept, &app.roots()),
                    TextStyle::xs(Token::Warning11),
                )
                .ellipsize(),
            ));
        }
    }
    Some(card)
}

fn receipt_state(state: &ChildState) -> (ToolState, &'static str) {
    let outcome = Outcome::of(state);
    let word = match outcome {
        Outcome::Running => "running",
        Outcome::Done => "completed",
        Outcome::Conflict => "conflict",
        Outcome::Kept => "kept",
        Outcome::Stopped => "aborted",
        Outcome::Failed => "failed",
        Outcome::Interrupted => "interrupted",
    };
    (outcome.tool_state(), word)
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
    roots.extend(app.roots().iter().cloned());
    let note = format!("{} · {word}", record.access.name());
    let card = sigil::prelude::column()
        .fill_width()
        .gap(Space::None)
        .child(header(
            &state,
            call_line(
                Builtin::Task.verb(),
                &Agent::title_of(&workspace_relative(&record.assignment, &roots)),
                Some((note, TextStyle::xs(Token::Gray10))),
                true,
            ),
        ));
    let Some(result) = record.state.result() else {
        return Some(card);
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
    let mut shown = preview::text(full, Builtin::Task.verb(), &body, failed, false);
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
