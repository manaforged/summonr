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
        patch: None,
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

#[test]
fn children_spawned_in_the_same_minute_keep_their_own_tokens() {
    let mut model = model();
    let ids = [
        "01a104ac00000000000000000000aaaa",
        "01a104ac00000000000000000000bbbb",
    ];
    for (id, tokens) in ids.iter().zip([100_u32, 7]) {
        model.apply_child(
            id,
            Event::Spawned {
                assignment: format!("job {id}"),
                access: crate::child::Access::Research,
                call: String::new(),
                background: false,
                workspace: String::new(),
                agent: String::new(),
            },
        );
        model.apply_child(
            id,
            Event::Usage {
                prompt_tokens: tokens,
                completion_tokens: 0,
                reasoning_tokens: 0,
                cached_tokens: 0,
                millis: None,
                prefix_hash: String::new(),
                cost: None,
            },
        );
    }
    let tokens: Vec<u64> = model.agents.iter().map(|agent| agent.tokens).collect();
    assert_eq!(tokens, [100, 7]);
}

#[test]
fn answer_stays_last_after_the_check_receipt_and_goal_verdict() {
    let mut model = model();
    model.apply(Msg::User("make the tests pass".to_string()));
    model.apply(Msg::Busy(Some(Activity::Turn)));
    model.apply(Msg::Event(Event::Delta {
        text: "Result: 4 passed.".to_string(),
    }));
    model.apply(Msg::Event(Event::Verify {
        command: "python3 -m pytest -q".to_string(),
        passed: true,
    }));
    model.apply(Msg::Event(Event::Assistant {
        text: "Result: 4 passed.".to_string(),
    }));
    model.apply(Msg::Receipt("Turn receipt · 3 files read".to_string()));
    model.apply(Msg::Outcome("goal achieved".to_string()));
    assert!(
        matches!(model.entries.iter().last(), Some(Entry::Assistant { text, .. }) if text == "Result: 4 passed."),
        "the answer must be the last row of the turn"
    );
    let held = model.commit_lines();
    assert!(!held.contains("4 passed."), "{held}");
    model.apply(Msg::Busy(None));
    let lines = held + &model.commit_lines();
    let verdict = lines
        .find("goal achieved")
        .expect("verdict reaches the scrollback");
    let answer = lines
        .find("4 passed.")
        .expect("answer reaches the scrollback");
    assert!(verdict < answer, "{lines}");
}

#[test]
fn journal_child_terminals_close_rows_once_without_recharging_usage() {
    use crate::child::{Access, ChildRecord, ChildState};
    use crate::durable::log::Line;
    let record = ChildRecord {
        version: 1,
        id: "01a104ac000000000000000000001111"
            .parse()
            .expect("child id"),
        parent: "01a104ac000000000000000000002222"
            .parse()
            .expect("parent id"),
        assignment: "Child terminal".to_owned(),
        model: "m".to_owned(),
        workspace: "/w".into(),
        depth: 1,
        access: Access::Research,
        background: false,
        call: Some("t1".to_owned()),
        state: ChildState::Running,
    };
    let interrupted = record.interrupt("cancelled by parent");
    let ChildState::Interrupted(mut result) = interrupted.state else {
        panic!("interrupted result")
    };
    result.text = "child answer".to_owned();
    result.usage.prompt_tokens = 2_000;
    let mut empty = result.clone();
    empty.text.clear();
    for (state, failed) in [
        (ChildState::Completed(result.clone()), false),
        (ChildState::Failed(result.clone()), true),
        (ChildState::Aborted(result.clone()), true),
        (ChildState::Interrupted(result), true),
        (ChildState::Completed(empty), false),
    ] {
        let mut model = model();
        model.apply(Msg::Busy(Some(Activity::Turn)));
        model.apply(Msg::Event(Event::Child {
            id: record.id.to_string(),
            event: Box::new(Event::Spawned {
                assignment: record.assignment.clone(),
                access: record.access,
                call: "t1".to_owned(),
                background: true,
                workspace: "/w".to_owned(),
                agent: String::new(),
            }),
        }));
        model.apply(Msg::Event(Event::Child {
            id: record.id.to_string(),
            event: Box::new(Event::Usage {
                prompt_tokens: 5,
                completion_tokens: 2,
                reasoning_tokens: 0,
                cached_tokens: 0,
                millis: None,
                prefix_hash: String::new(),
                cost: Some(0.25),
            }),
        }));
        let line = Line::Child {
            record: ChildRecord {
                state,
                ..record.clone()
            },
        };
        for _ in 0..2 {
            for event in line.events() {
                model.apply(Msg::Event(event));
            }
        }
        assert!(model.agents[0].done);
        assert_eq!(model.agents[0].failed, failed);
        assert_eq!(model.agents[0].tokens, 7);
        assert_eq!(model.agents[0].cost, 0.25);
        assert!(model.busy_since.is_some());
        assert_eq!(
            model
                .entries
                .iter()
                .filter(|entry| matches!(entry, Entry::Tool { name, .. } if name == "Subagent"))
                .count(),
            1
        );
    }
}
