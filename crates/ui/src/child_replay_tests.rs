use super::*;
use summonr_agent::child::{Access, ChildRecord, ChildState};
use summonr_agent::durable::log::Line;

fn worker() -> ChildRecord {
    ChildRecord {
        version: 1,
        id: "01a104ac000000000000000000001111"
            .parse()
            .expect("child id"),
        parent: "01a104ac000000000000000000003333"
            .parse()
            .expect("parent id"),
        assignment: "read the assignment".to_string(),
        model: "m".to_string(),
        name: "worker".to_string(),
        agent: String::new(),
        workspace: "/workspace".into(),
        depth: 1,
        access: Access::Research,
        background: true,
        call: Some("task-call".to_string()),
        base: String::new(),
        state: ChildState::Running,
    }
}

fn replay(model: &mut Model, record: ChildRecord) {
    let bytes = serde_json::to_vec(&Line::Child { record }).expect("journal record serializes");
    let line: Line = serde_json::from_slice(&bytes).expect("journal record deserializes");
    for event in line.events() {
        model.apply(Msg::Fact(Fact::Event(event)));
    }
}

fn status(model: &Model) -> Vec<(String, String, bool, bool, String)> {
    model
        .agents
        .iter()
        .map(|agent| {
            (
                agent.id.clone(),
                agent.name.clone(),
                agent.done,
                agent.failed,
                agent.last.clone(),
            )
        })
        .collect()
}

#[test]
fn a_buffered_recovery_has_the_same_healthy_status_live_and_after_replay() {
    let record = worker();
    let id = record.id.to_string();
    let ChildState::Interrupted(mut result) = record.interrupt("first request rejected").state
    else {
        panic!("the fixture carries a terminal result");
    };
    result.text = "recovered".to_string();
    result.diagnostic = None;
    let mut live = model();
    live.apply(Msg::Fact(Fact::Event(record.spawned())));
    live.apply(Msg::Fact(Fact::Event(Event::Child {
        id: id.clone(),
        event: Box::new(Event::Aborted {
            reason: "first request rejected".to_string(),
            code: summonr_agent::tools::AbortCode::Error,
        }),
    })));
    assert!(matches!(live.agents[0].state(), ToolState::Failed));
    live.apply(Msg::Fact(Fact::Event(Event::Child {
        id,
        event: Box::new(Event::Assistant {
            text: "recovered".to_string(),
        }),
    })));
    let mut restored = model();
    replay(&mut restored, record.clone());
    replay(
        &mut restored,
        ChildRecord {
            state: ChildState::Completed(result),
            ..record
        },
    );
    assert_eq!(live.agents.len(), 1);
    assert!(matches!(live.agents[0].state(), ToolState::Ok));
    assert_eq!(live.agents[0].last, "recovered");
    assert_eq!(status(&live), status(&restored));
}

#[test]
fn replay_retains_failed_attempts_without_merging_workers_with_the_same_name() {
    let first = worker();
    let ChildState::Interrupted(mut failed) = first.interrupt("request rejected").state else {
        panic!("the fixture carries a terminal result");
    };
    failed.text.clear();
    let mut second = first.clone();
    second.id = "01a104ac000000000000000000002222"
        .parse()
        .expect("second child id");
    let mut completed = failed.clone();
    completed.text = "replacement completed".to_string();
    completed.diagnostic = None;
    let mut live = model();
    for record in [&first, &second] {
        live.apply(Msg::Fact(Fact::Event(record.spawned())));
    }
    live.apply(Msg::Fact(Fact::Event(Event::Child {
        id: first.id.to_string(),
        event: Box::new(Event::Aborted {
            reason: "request rejected".to_string(),
            code: summonr_agent::tools::AbortCode::Error,
        }),
    })));
    live.apply(Msg::Fact(Fact::Event(Event::Child {
        id: second.id.to_string(),
        event: Box::new(Event::Assistant {
            text: "replacement completed".to_string(),
        }),
    })));
    let mut restored = model();
    replay(
        &mut restored,
        ChildRecord {
            state: ChildState::Failed(failed),
            ..first
        },
    );
    replay(
        &mut restored,
        ChildRecord {
            state: ChildState::Completed(completed),
            ..second
        },
    );
    assert_eq!(live.agents.len(), 2);
    assert!(matches!(live.agents[0].state(), ToolState::Failed));
    assert!(matches!(live.agents[1].state(), ToolState::Ok));
    assert_eq!(status(&live), status(&restored));
}
