use super::{fixture, reducer};
use crate::durable::context::{ContextEntry, ContextRole, ContextSource};
use crate::durable::effect::{
    MeterKind, MeterSettlement, ToolCancellation, ToolOutcome, ToolResult, Usage,
};
use crate::durable::fact::{Fact, FactData, MeterOwner};
use crate::durable::id::Digest;
use crate::durable::journal::{JournalWriter, inspect};
use crate::durable::workspace::WorkspaceEffect;

#[test]
fn prestart_tool_terminals_have_no_attempt_or_meter() {
    let cases = [
        (
            "denied",
            ToolOutcome::Denied {
                diagnostic: fixture::text("policy denied the call"),
            },
        ),
        (
            "unavailable",
            ToolOutcome::Unavailable {
                diagnostic: fixture::text("recorded registration is unavailable"),
            },
        ),
        (
            "cancelled",
            ToolOutcome::Cancelled {
                state: ToolCancellation::Prestart,
            },
        ),
    ];
    for (label, outcome) in cases {
        let directory = tempfile::tempdir().expect("temporary directory must exist");
        let path = directory.path().join(format!("{label}.jsonl"));
        let mut writer = planned(&path);
        writer
            .append(
                fixture::id(5),
                None,
                vec![
                    Fact {
                        id: fixture::fact_id(400),
                        data: FactData::ToolFinished {
                            result: ToolResult {
                                id: fixture::id(302),
                                call: fixture::id(300),
                                attempt: None,
                                outcome,
                                workspace: WorkspaceEffect::None,
                            },
                        },
                    },
                    Fact {
                        id: fixture::fact_id(401),
                        data: FactData::ContextEntryAdded {
                            entry: tool_entry(label),
                        },
                    },
                ],
            )
            .expect("pre-start terminal group must append");
        assert_eq!(
            inspect(&path).expect("pre-start terminal must replay"),
            *writer.state()
        );
    }
}

#[test]
fn started_failure_and_stopped_cancel_settle_once() {
    let cases = [
        (
            "failed",
            ToolOutcome::Failed {
                diagnostic: fixture::text("tool failed"),
            },
        ),
        (
            "stopped",
            ToolOutcome::Cancelled {
                state: ToolCancellation::Stopped,
            },
        ),
    ];
    for (label, outcome) in cases {
        let directory = tempfile::tempdir().expect("temporary directory must exist");
        let path = directory.path().join(format!("{label}.jsonl"));
        let mut writer = planned(&path);
        reducer::start_tool(&mut writer);
        writer
            .append(
                fixture::id(6),
                None,
                vec![
                    Fact {
                        id: fixture::fact_id(400),
                        data: FactData::ToolFinished {
                            result: ToolResult {
                                id: fixture::id(302),
                                call: fixture::id(300),
                                attempt: Some(fixture::id(304)),
                                outcome,
                                workspace: WorkspaceEffect::None,
                            },
                        },
                    },
                    Fact {
                        id: fixture::fact_id(401),
                        data: FactData::MeterSettled {
                            meter: fixture::id(305),
                            owner: MeterOwner::Tool(fixture::id(304)),
                            kind: MeterKind::Tool,
                            settlement: MeterSettlement::Known(usage()),
                        },
                    },
                    Fact {
                        id: fixture::fact_id(402),
                        data: FactData::ContextEntryAdded {
                            entry: tool_entry(label),
                        },
                    },
                ],
            )
            .expect("started terminal group must append");
        assert_eq!(
            inspect(&path).expect("started terminal must replay"),
            *writer.state()
        );
    }
}

fn planned(path: &Path) -> JournalWriter {
    let mut writer = fixture::writer(path);
    reducer::start_operation(&mut writer);
    reducer::start_provider(&mut writer);
    reducer::finish_provider_with_tool(&mut writer);
    writer
}

fn tool_entry(label: &str) -> ContextEntry {
    let mut entry = ContextEntry {
        id: fixture::id(403),
        epoch: fixture::epoch_id(),
        role: ContextRole::Tool,
        source: ContextSource::Tool(fixture::id(302)),
        payload: fixture::text(label),
        digest: Digest::default(),
    };
    entry.digest = entry.calculated_digest();
    entry
}

fn usage() -> Usage {
    Usage {
        input_tokens: Some(0),
        output_tokens: Some(0),
        cached_input_tokens: Some(0),
        live_millis: Some(10),
    }
}
use std::path::Path;
