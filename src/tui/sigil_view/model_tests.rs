use crate::tools::Event;
use crate::tui::{Activity, Msg, ToolState};

use super::{Entry, Model};

fn model() -> Model {
    Model::new(&super::super::profile::info())
}

fn call(model: &mut Model, id: &str, name: &str) {
    model.apply(Msg::Event(Event::ToolCall {
        id: id.to_string(),
        name: name.to_string(),
        arguments: "{}".to_string(),
    }));
}

fn finish(model: &mut Model, id: &str) {
    model.apply(Msg::Event(Event::ToolResult {
        id: id.to_string(),
        success: true,
        mutated: false,
        output: format!("output {id}"),
        millis: None,
    }));
}

fn singles(model: &Model) -> usize {
    model
        .entries
        .iter()
        .filter(|entry| matches!(entry, Entry::Tool { .. }))
        .count()
}

fn group_sizes(model: &Model) -> Vec<usize> {
    model
        .entries
        .iter()
        .filter_map(|entry| {
            if let Entry::ToolGroup { calls, .. } = entry {
                Some(calls.len())
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn repeated_reads_fold_into_one_group_and_each_call_finishes() {
    let mut model = model();
    for id in ["1", "2", "3"] {
        call(&mut model, id, "read");
        finish(&mut model, id);
    }
    assert_eq!(group_sizes(&model), vec![3]);
    assert_eq!(singles(&model), 0);
    let finished = model.entries.iter().any(|entry| {
        matches!(entry, Entry::ToolGroup { calls, .. }
            if calls.iter().all(|call| matches!(call.state, ToolState::Ok) && call.output.starts_with("output")))
    });
    assert!(finished, "every grouped call must carry its own result");
}

#[test]
fn edits_never_fold_into_a_group() {
    let mut model = model();
    for id in ["1", "2"] {
        call(&mut model, id, "edit");
        finish(&mut model, id);
    }
    assert!(group_sizes(&model).is_empty());
    assert_eq!(singles(&model), 2);
}

#[test]
fn inline_commits_each_settled_entry_once_and_restarts_after_reset() {
    let mut model = model();
    model.apply(Msg::User("hello".to_string()));
    let first = model.commit_lines();
    assert!(first.contains("hello"), "{first}");
    assert_eq!(
        model.commit_lines(),
        "",
        "an entry must reach the scrollback once"
    );

    model.apply(Msg::Busy(Some(Activity::Turn)));
    call(&mut model, "7", "search");
    assert_eq!(
        model.commit_lines(),
        "",
        "a running tool stays in the live region"
    );
    finish(&mut model, "7");
    model.apply(Msg::Busy(None));
    assert!(model.commit_lines().contains("search"));

    model.apply(Msg::Reset);
    model.apply(Msg::User("again".to_string()));
    assert!(model.commit_lines().contains("again"));
}
