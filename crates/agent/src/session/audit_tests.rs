use std::path::{Path, PathBuf};

use crate::durable::log::{self, Line, Log, Usage};
use crate::durable::store::Durability;
use crate::mail::Mailbox;
use serde_json::{Value, json};
use summonr_types::Continuation;

const COORDINATOR: &str = "000000000000000018d2297ad34281e1";
const WORKER: &str = "000000000000000018d2297ad34281e2";

#[test]
fn native_messages_and_provider_records_reach_the_audit_export() {
    let home = tempfile::tempdir().expect("audit home");
    journal(home.path(), COORDINATOR);
    journal(home.path(), WORKER);
    let mail = home.path().join("mail");
    let [mut coordinator, mut worker] =
        [(COORDINATOR, "coordinator"), (WORKER, "worker")].map(|(id, name)| {
            Mailbox::open(&mail, id, Some(name), "summonr", "model", home.path()).expect("mailbox")
        });
    let (_, request) = coordinator
        .send("worker", "measure candidate", None)
        .expect("send request");
    assert_eq!(worker.poll().expect("worker inbox").len(), 1);
    let (_, reply) = worker
        .send("coordinator", "evidence ready", Some(&request))
        .expect("reply");
    assert_eq!(coordinator.poll().expect("coordinator inbox").len(), 1);
    let records: Vec<Value> = [COORDINATOR, WORKER]
        .into_iter()
        .flat_map(|id| replayable(home.path(), id))
        .collect();
    assert_eq!(records.len(), 10);
    let stage = |session: &str, kind: &str, from: &str, to: &str, reply_to: Value, text: &str| {
        json!([session, kind, session, from, to, reply_to, text])
    };
    assert_eq!(
        stages(&records, &request),
        [
            stage(
                COORDINATOR,
                "send_attempted",
                COORDINATOR,
                WORKER,
                Value::Null,
                "measure candidate"
            ),
            stage(
                COORDINATOR,
                "delivered",
                COORDINATOR,
                WORKER,
                Value::Null,
                "measure candidate"
            ),
            stage(
                WORKER,
                "dequeued",
                COORDINATOR,
                WORKER,
                Value::Null,
                "measure candidate"
            ),
        ]
    );
    assert_eq!(
        stages(&records, &reply),
        [
            stage(
                WORKER,
                "send_attempted",
                WORKER,
                COORDINATOR,
                json!(request),
                "evidence ready"
            ),
            stage(
                WORKER,
                "delivered",
                WORKER,
                COORDINATOR,
                json!(request),
                "evidence ready"
            ),
            stage(
                COORDINATOR,
                "dequeued",
                WORKER,
                COORDINATOR,
                json!(request),
                "evidence ready"
            ),
        ]
    );
    assert_eq!(
        responses(&records),
        [COORDINATOR, WORKER].map(|id| json!([
            id,
            "fixture-model",
            "provider summary",
            11,
            7,
            3,
            5,
            "provider-only"
        ]))
    );
}

#[test]
fn only_a_complete_export_ends_with_the_trailer_that_counts_its_records() {
    let home = tempfile::tempdir().expect("audit home");
    journal(home.path(), WORKER);
    let bytes = std::fs::read(exported(home.path(), WORKER)).expect("export");
    assert_eq!(complete(&bytes).len(), 2);
    let mut interrupted = lines(&bytes);
    interrupted.pop().expect("completion manifest");
    assert!(
        interrupted
            .iter()
            .all(|record| record["record"]["kind"] != "complete_prefix"),
        "{interrupted:?}"
    );
}

fn replayable(home: &Path, id: &str) -> Vec<Value> {
    let first = std::fs::read(exported(home, id)).expect("export");
    let again = std::fs::read(exported(home, id)).expect("replayed export");
    assert_eq!(first, again, "an export replays byte for byte");
    complete(&first)
}

fn lines(bytes: &[u8]) -> Vec<Value> {
    std::str::from_utf8(bytes)
        .expect("export UTF-8")
        .lines()
        .map(|line| serde_json::from_str(line).expect("export JSON"))
        .collect()
}

fn complete(bytes: &[u8]) -> Vec<Value> {
    let mut records = lines(bytes);
    let trailer = records.pop().expect("completion manifest");
    assert_eq!(trailer["source"], "export", "{trailer}");
    assert_eq!(trailer["record"]["kind"], "complete_prefix", "{trailer}");
    assert_eq!(trailer["record"]["records"], records.len(), "{trailer}");
    records
}

const LIFECYCLE: [&str; 3] = ["send_attempted", "delivered", "dequeued"];

fn stages(records: &[Value], id: &str) -> Vec<Value> {
    let mut mail: Vec<&Value> = records
        .iter()
        .filter(|record| record["source"] == "mail" && record["record"]["mail"]["id"] == id)
        .collect();
    mail.sort_by_key(|record| {
        LIFECYCLE
            .iter()
            .position(|kind| record["record"]["kind"] == *kind)
    });
    mail.into_iter()
        .map(|record| {
            let (body, mail) = (&record["record"], &record["record"]["mail"]);
            json!([
                record["session"],
                body["kind"],
                body["actor"],
                mail["from"],
                mail["to"],
                mail["reply_to"],
                mail["text"]
            ])
        })
        .collect()
}

fn responses(records: &[Value]) -> Vec<Value> {
    records
        .iter()
        .filter(|record| record["source"] == "session" && record["record"]["kind"] == "assistant")
        .map(|record| {
            let (body, usage) = (&record["record"], &record["record"]["usage"]);
            json!([
                record["session"],
                body["model"],
                body["reasoning"],
                usage["input"],
                usage["output"],
                usage["reasoning"],
                usage["cached"],
                body["continuation"]["signatures"][0]
            ])
        })
        .collect()
}

fn journal(home: &Path, id: &str) {
    let path = log::sessions_dir(home).join(id).join("session.jsonl");
    let mut log = Log::create(&path).expect("native session journal");
    log.append(
        &Line::Session {
            id: id.parse().expect("session id"),
            model: "fixture-model".to_string(),
            system: "fixture system".to_string(),
            workspace: home.to_path_buf(),
            parent: None,
            depth: 0,
            revision: Some("fixture-build".to_string()),
            interactive: false,
        },
        Durability::Synced,
    )
    .expect("session header");
    log.append(
        &Line::Assistant {
            text: "provider answer".to_string(),
            reasoning: "provider summary".to_string(),
            tool_calls: Vec::new(),
            usage: Usage {
                input: 11,
                output: 7,
                reasoning: 3,
                cached: 5,
                written: 0,
                millis: 4,
            },
            model: "fixture-model".to_string(),
            prefix_hash: "request-prefix".to_string(),
            cost: Some(0.125),
            continuation: Some(Continuation::Responses {
                model: "fixture-model".to_string(),
                signatures: vec!["provider-only".to_string()],
                phase: Some("final".to_string()),
                commentary: vec!["provider commentary".to_string()],
            }),
        },
        Durability::Synced,
    )
    .expect("provider record");
}

fn exported(home: &Path, id: &str) -> PathBuf {
    let source = log::sessions_dir(home).join(id).join("session.jsonl");
    let destination = home.join(format!("{id}.audit.jsonl"));
    let mut bytes = Vec::new();
    crate::export_json(&source, &mut bytes).expect("native audit export");
    std::fs::write(&destination, bytes).expect("export snapshot");
    destination
}
