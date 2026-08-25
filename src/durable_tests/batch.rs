use super::{fixture, reducer};
use crate::durable::context::{ContextRole, ContextSource};
use crate::durable::effect::{MeterSettlement, ToolIntent, ToolOutcome, ToolResult};
use crate::durable::fact::{Fact, FactData, MeterOwner};
use crate::durable::journal::{JournalWriter, inspect};

#[test]
fn multi_tool_batch_projects_provider_order_after_out_of_order_completion() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut writer = fixture::writer(&path);
    reducer::start_operation(&mut writer);
    reducer::start_provider(&mut writer);

    let (first_payload, first_call) = reducer::planned_call(0, 300, 301, 302, "call-1");
    let (second_payload, second_call) = reducer::planned_call(1, 310, 301, 312, "call-2");
    writer
        .append(
            fixture::id(4),
            None,
            reducer::provider_facts(
                vec![first_payload, second_payload],
                vec![first_call, second_call],
            ),
        )
        .expect("one ordered tool batch must append");
    start(&mut writer, 5, 20, 314, 310);
    start(&mut writer, 6, 21, 304, 300);

    writer
        .append(fixture::id(7), None, terminal(22, 312, 310, 314, "second"))
        .expect("second call can finish before the first");
    assert_eq!(
        inspect(&path).expect("partial batch prefix must replay"),
        *writer.state()
    );

    let mut reversed = terminal(24, 302, 300, 304, "first");
    reversed.push(context(26, 318, 312, "second"));
    reversed.push(context(27, 319, 302, "first"));
    writer
        .append(fixture::id(8), None, reversed)
        .expect_err("provider context cannot follow completion order");

    let mut ordered = terminal(24, 302, 300, 304, "first");
    ordered.push(context(26, 318, 302, "first"));
    ordered.push(context(27, 319, 312, "second"));
    writer
        .append(fixture::id(8), None, ordered)
        .expect("provider context must follow provider-call order");
    assert_eq!(
        inspect(&path).expect("closed batch must replay"),
        *writer.state()
    );
}

#[test]
fn partial_batch_result_cannot_enter_provider_context() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut writer = fixture::writer(&path);
    reducer::start_operation(&mut writer);
    reducer::start_provider(&mut writer);
    let (first_payload, first_call) = reducer::planned_call(0, 300, 301, 302, "call-1");
    let (second_payload, second_call) = reducer::planned_call(1, 310, 301, 312, "call-2");
    writer
        .append(
            fixture::id(4),
            None,
            reducer::provider_facts(
                vec![first_payload, second_payload],
                vec![first_call, second_call],
            ),
        )
        .expect("one ordered tool batch must append");
    start(&mut writer, 5, 20, 314, 310);

    let mut facts = terminal(21, 312, 310, 314, "second");
    facts.push(context(23, 318, 312, "second"));
    writer
        .append(fixture::id(6), None, facts)
        .expect_err("partial batch result must remain pending");
}

fn start(writer: &mut JournalWriter, commit: u128, fact: u128, attempt: u128, call: u128) {
    writer
        .append(
            fixture::id(commit),
            None,
            vec![Fact {
                id: fixture::fact_id(fact),
                data: FactData::ToolStarted {
                    intent: ToolIntent {
                        attempt: fixture::id(attempt),
                        call: fixture::id(call),
                        normalized_arguments: fixture::text(r#"{"path":"README.md"}"#),
                    },
                },
            }],
        )
        .expect("tool attempt must start");
}

fn terminal(fact: u128, result: u128, call: u128, attempt: u128, output: &str) -> Vec<Fact> {
    vec![
        Fact {
            id: fixture::fact_id(fact),
            data: FactData::ToolFinished {
                result: ToolResult {
                    id: fixture::id(result),
                    call: fixture::id(call),
                    attempt: Some(fixture::id(attempt)),
                    outcome: ToolOutcome::Completed {
                        output: fixture::text(output),
                    },
                },
            },
        },
        Fact {
            id: fixture::fact_id(fact + 1),
            data: FactData::MeterSettled {
                owner: MeterOwner::Tool(fixture::id(attempt)),
                settlement: MeterSettlement::Known(reducer::usage()),
            },
        },
    ]
}

fn context(fact: u128, entry: u128, result: u128, payload: &str) -> Fact {
    Fact {
        id: fixture::fact_id(fact),
        data: FactData::ContextEntryAdded {
            entry: reducer::context_entry(
                entry,
                ContextRole::Tool,
                ContextSource::Tool(fixture::id(result)),
                payload,
            ),
        },
    }
}
