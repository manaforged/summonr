use sigil::prelude::{AlignItems, NodeBuilder, Widget, container};

use super::super::style::{ACTIVE_DOT, DONE_DOT, FAILED_DOT, L, S, Text, XS, text};
use crate::app::App;
use crate::model::Agent;
use crate::model::Entry;
use crate::{Outcome, ToolState};
use summonr_agent::child::{ChildRecord, ChildState};

struct Row {
    title: String,
    state: ToolState,
    word: &'static str,
    result: Option<String>,
}

/// The subagents a run of task calls started, one plain row each.
pub(crate) fn group(app: &App, range: std::ops::Range<usize>) -> NodeBuilder {
    let rows = app.model.with(|model| {
        range
            .filter_map(|at| model.entries.get(at).cloned())
            .flat_map(|entry| rows_of(&entry, |call| model.agents_of(call).cloned().collect()))
            .collect::<Vec<_>>()
    });
    rows.into_iter().fold(
        sigil::prelude::column()
            .fill_width()
            .gap(S)
            .min_width(0.0)
            .min_height(0.0),
        |column, row| column.child(line(row)),
    )
}

fn rows_of(entry: &Entry, live: impl Fn(&str) -> Vec<Agent>) -> Vec<Row> {
    let Entry::Tool { calls, .. } = entry else {
        return Vec::new();
    };
    calls
        .iter()
        .flat_map(
            |call| match serde_json::from_str::<ChildRecord>(&call.output) {
                Ok(record) => vec![finished(&record)],
                Err(_) => live(&call.id).iter().map(running).collect(),
            },
        )
        .collect()
}

fn finished(record: &ChildRecord) -> Row {
    let outcome = Outcome::of(&record.state);
    let word = match outcome {
        Outcome::Running => "Running",
        Outcome::Done => "Done",
        Outcome::Conflict => "Conflict",
        Outcome::Kept => "Kept",
        Outcome::Stopped => "Stopped",
        Outcome::Failed => "Failed",
        Outcome::Interrupted => "Interrupted",
    };
    let result = match &record.state {
        ChildState::Running => None,
        ChildState::Completed(result)
        | ChildState::Aborted(result)
        | ChildState::Failed(result)
        | ChildState::Interrupted(result) => Some(result),
    };
    let result = result.and_then(|result| {
        let text = if result.text.trim().is_empty() {
            result.diagnostic.clone().unwrap_or_default()
        } else {
            result.text.clone()
        };
        text.lines()
            .find(|line| !line.trim().is_empty())
            .map(str::to_string)
    });
    Row {
        title: Agent::title_of(&record.assignment),
        state: outcome.tool_state(),
        word,
        result,
    }
}

fn running(kid: &Agent) -> Row {
    let title = if kid.assignment.is_empty() {
        kid.title.clone()
    } else {
        Agent::title_of(&kid.assignment)
    };
    let (word, state) = match kid.state() {
        ToolState::Running => ("Running", ToolState::Running),
        ToolState::Ok => ("Done", ToolState::Ok),
        ToolState::Failed => ("Failed", ToolState::Failed),
    };
    Row {
        title,
        state,
        word,
        result: None,
    }
}

fn line(row: Row) -> NodeBuilder {
    let head = sigil::prelude::row()
        .fill_width()
        .gap(S)
        .min_width(0.0)
        .min_height(0.0)
        .align_items(AlignItems::Center)
        .child(dot(row.state))
        .child(
            text(Text::Label, row.title)
                .ellipsize()
                .grow(1.0)
                .shrink(1.0)
                .min_width(0.0)
                .min_height(0.0),
        )
        .child(text(Text::Meta, row.word).nowrap());
    let column = sigil::prelude::column()
        .fill_width()
        .gap(XS)
        .min_width(0.0)
        .min_height(0.0)
        .child(head);
    match row.result {
        Some(result) => column.child(
            container()
                .fill_width()
                .padding_left(L)
                .min_width(0.0)
                .min_height(0.0)
                .child(text(Text::Meta, result).ellipsize()),
        ),
        None => column,
    }
}

fn dot(state: ToolState) -> NodeBuilder {
    super::super::style::dot(match state {
        ToolState::Running => ACTIVE_DOT,
        ToolState::Ok => DONE_DOT,
        ToolState::Failed => FAILED_DOT,
    })
}

#[cfg(test)]
#[path = "agents_tests.rs"]
mod tests;
