use std::path::Path;
use std::time::Duration;

use serde_json::Value;

use crate::support::{Model, Session, Step, read, slow};

fn logging(log: &Path, events: &[&str]) -> String {
    let command = format!("cat >> '{0}'; echo >> '{0}'", log.display());
    let command = serde_json::to_string(&command).expect("quote the hook command");
    let mut config = String::from("[hooks]\n");
    for event in events {
        config.push_str(&format!("{event} = {command}\n"));
    }
    config
}

fn logged(log: &Path) -> Vec<Value> {
    read(log)
        .lines()
        .map(|line| serde_json::from_str(line).expect("a hook logs one JSON object per line"))
        .collect()
}

fn named(seen: &[Value]) -> Vec<&str> {
    seen.iter()
        .map(|event| event["event"].as_str().unwrap_or_default())
        .collect()
}

pub(crate) fn a_print_run_opens_and_closes_its_session_hooks() {
    let session = Session::new();
    let log = session.file("hooks.jsonl");
    let config = logging(
        &log,
        &["session_start", "user_prompt", "stop", "session_end"],
    );
    let model = Model::start(vec![Step::Text("ok")]);
    session.run(&model, &config, &[]);
    let seen = logged(&log);
    assert_eq!(
        named(&seen),
        ["session_start", "user_prompt", "stop", "session_end"],
        "{seen:?}"
    );
    assert_eq!(seen[0]["source"], "startup", "{seen:?}");
    assert_eq!(seen[3]["reason"], "other", "{seen:?}");
}

pub(crate) fn a_turn_cut_by_the_time_limit_still_runs_stop() {
    let session = Session::new();
    let log = session.file("hooks.jsonl");
    let config = logging(&log, &["stop", "stop_failure", "session_end"]);
    let model = Model::start(vec![Step::After(
        Duration::from_secs(slow(6)),
        Box::new(Step::Text("too late")),
    )]);
    let limit = slow(2).to_string();
    session.run(&model, &config, &["--max-secs", &limit]);
    let seen = logged(&log);
    assert_eq!(named(&seen), ["stop", "session_end"], "{seen:?}");
    assert_eq!(seen[0]["aborted"], true, "{seen:?}");
}
