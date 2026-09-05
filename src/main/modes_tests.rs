use super::{exit, slash};
use summoner::{Turn, Usage, WorkKind, WorkPhase, WorkProgress};

#[test]
fn a_leading_slash_line_is_a_command_not_a_prompt() {
    assert_eq!(slash("/exit\n"), Some("/exit"));
    assert_eq!(slash("  /model grok"), Some("/model"));
    assert_eq!(slash("fix /tmp/x"), None);
    assert_eq!(slash("/var/log is full on the box, why"), None);
    assert_eq!(slash("/Users/x/repo has a bug"), None);
    assert_eq!(slash(""), None);
}

fn turn() -> Turn {
    Turn {
        text: "done".to_string(),
        events: Vec::new(),
        rounds: 1,
        usage: Usage::default(),
        checks: Vec::new(),
        aborted: false,
    }
}

fn progress(phase: WorkPhase) -> WorkProgress {
    WorkProgress {
        kind: WorkKind::Single,
        objective: String::new(),
        phase,
        checks_failed: false,
        current: None,
        total: 0,
        attempts: 0,
        rounds: 0,
        max_turns: 0,
        reason: None,
        ledger: Vec::new(),
        touched: Vec::new(),
    }
}

#[test]
fn headless_exit_stops_short_when_work_is_deferred() {
    assert_eq!(exit(&turn(), &progress(WorkPhase::Execute)), 3);
    assert_eq!(exit(&turn(), &progress(WorkPhase::Completed)), 0);
    assert_eq!(exit(&turn(), &progress(WorkPhase::Plan)), 0);
}
