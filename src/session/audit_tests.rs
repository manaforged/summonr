use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::durable::log::{self, Line, Log, Usage};
use crate::durable::store::Durability;
use crate::mail::Mailbox;
use serde_json::{Value, json};
use summonr_types::Continuation;

const COORDINATOR: &str = "000000000000000018d2297ad34281e1";
const WORKER: &str = "000000000000000018d2297ad34281e2";

#[test]
fn native_messages_and_provider_records_reach_the_sqlite_audit_log() {
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
    let db = home.path().join("audit.sqlite");
    for id in [COORDINATOR, WORKER] {
        let export = exported(home.path(), id);
        succeeded(cli(&db).arg("import").arg("--file").arg(&export));
        let replay = succeeded(cli(&db).arg("import").arg("--file").arg(&export));
        assert_eq!(replay["inserted"], 0);
    }
    let saved = query(&db, &request, &reply);
    verify(&saved, &request);
}

fn verify(saved: &Value, request: &str) {
    assert_eq!(
        saved["request"],
        json!([[COORDINATOR, WORKER, null, "measure candidate", 3, 3]])
    );
    assert_eq!(
        saved["reply"],
        json!([[WORKER, COORDINATOR, request, "evidence ready", 3, 3]])
    );
    assert_eq!(
        saved["responses"],
        json!([
            [
                COORDINATOR,
                "fixture-model",
                "provider summary",
                11,
                7,
                3,
                5
            ],
            [WORKER, "fixture-model", "provider summary", 11, 7, 3, 5],
        ])
    );
    assert_eq!(
        saved["actors"],
        json!([
            [COORDINATOR, "delivered"],
            [WORKER, "dequeued"],
            [COORDINATOR, "send_attempted"],
        ])
    );
    assert_eq!(
        saved["opaque"],
        json!([["provider-only"], ["provider-only"]])
    );
    assert_eq!(saved["events"], 12);
}

#[test]
fn an_incomplete_native_export_cannot_be_reported_as_a_complete_import() {
    let home = tempfile::tempdir().expect("audit home");
    journal(home.path(), WORKER);
    let db = home.path().join("audit.sqlite");
    let export = exported(home.path(), WORKER);
    let mut lines: Vec<String> = std::fs::read_to_string(&export)
        .expect("export")
        .lines()
        .map(str::to_string)
        .collect();
    lines.pop().expect("completion manifest");
    std::fs::write(&export, format!("{}\n", lines.join("\n"))).expect("interrupted export");
    let failed = cli(&db)
        .arg("import")
        .arg("--file")
        .arg(&export)
        .output()
        .expect("import process");
    assert!(!failed.status.success());
    let error: Value = serde_json::from_slice(&failed.stderr).expect("structured import failure");
    assert!(
        error["error"]
            .as_str()
            .expect("diagnostic")
            .contains("trailer missing"),
        "{error}"
    );
    let complete = exported(home.path(), WORKER);
    let recovered = succeeded(cli(&db).arg("import").arg("--file").arg(complete));
    assert_eq!(recovered["complete_export"], true);
    assert_eq!(query(&db, "unused", "unused")["events"], 3);
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

fn cli(db: &Path) -> Command {
    let mut command =
        Command::new(Path::new(env!("CARGO_MANIFEST_DIR")).join("plugins/lanes/bin/lanes"));
    command.arg("--db").arg(db);
    command
}

fn succeeded(command: &mut Command) -> Value {
    let output = command.output().expect("plugin command");
    decoded(output)
}

fn decoded(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8(output.stderr).expect("UTF-8 diagnostic")
    );
    serde_json::from_slice(&output.stdout).expect("command JSON")
}

fn query(db: &Path, request: &str, reply: &str) -> Value {
    let script = r#"
import json, sqlite3, sys
with sqlite3.connect(sys.argv[1]) as db:
    sql = 'select sender,recipient,reply_to,text,count(*),count(distinct stage) from message where id=? group by sender,recipient,reply_to,text'
    result = {
        'request': db.execute(sql, (sys.argv[2],)).fetchall(),
        'reply': db.execute(sql, (sys.argv[3],)).fetchall(),
        'actors': db.execute('select actor,stage from message where id=? order by stage', (sys.argv[2],)).fetchall(),
        'responses': db.execute("select session,model,reasoning,json_extract(usage,'$.input'),json_extract(usage,'$.output'),json_extract(usage,'$.reasoning'),json_extract(usage,'$.cached') from response order by session").fetchall(),
        'opaque': db.execute("select json_extract(body,'$.record.continuation.signatures[0]') from event where source='session' and kind='assistant' order by session").fetchall(),
        'events': db.execute('select count(*) from event').fetchone()[0],
    }
print(json.dumps(result))
"#;
    decoded(
        Command::new("python3")
            .arg("-c")
            .arg(script)
            .arg(db)
            .arg(request)
            .arg(reply)
            .output()
            .expect("SQLite query"),
    )
}
