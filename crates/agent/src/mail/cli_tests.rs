use super::*;
use crate::mail::Mailbox;

const SENDER: &str = "000000000000000018d2297ad34281e1";
const RECEIVER: &str = "000000000000000018d2297ad34281e2";

fn peers(dir: &Path) -> (Mailbox, Mailbox) {
    let sender = Mailbox::open(dir, SENDER, Some("sender"), "summonr", "model", dir)
        .expect("sender mailbox");
    let receiver = Mailbox::open(dir, RECEIVER, Some("receiver"), "summonr", "model", dir)
        .expect("receiver mailbox");
    (sender, receiver)
}

#[test]
fn cli_replies_return_structured_message_ids() {
    let home = tempfile::tempdir().expect("mail home");
    let (mut sender, mut receiver) = peers(home.path());
    let (_, request) = sender
        .send("receiver", "measure the candidate", None)
        .expect("request");
    assert_eq!(receiver.poll().expect("receive request").len(), 1);
    let shown = answer(
        home.path(),
        home.path(),
        &[
            "send",
            "--reply-to",
            &request,
            "--json",
            "sender",
            "measurement finished",
        ],
    )
    .expect("CLI reply receipt");
    let receipt: serde_json::Value = serde_json::from_str(&shown).expect("JSON receipt");
    let reply = sender.poll().expect("receive reply");
    assert_eq!(receipt["delivered"], 1);
    assert_eq!(reply.len(), 1);
    assert_eq!(receipt["id"], reply[0].id);
    assert_eq!(reply[0].reply_to.as_deref(), Some(request.as_str()));
    assert_eq!(reply[0].to, SENDER);
    assert_eq!(reply[0].text, "measurement finished");
    std::fs::remove_file(receiver.record_path()).expect("expire sender discovery record");
    receiver
        .send("sender", "native follow-up", Some(&request))
        .expect("native follow-up after discovery expires");
    let follow_up = sender.poll().expect("receive native follow-up");
    assert_eq!(follow_up.len(), 1);
    assert_eq!(follow_up[0].name, "receiver");
    assert_eq!(follow_up[0].kind, "summonr");
}

#[test]
fn clearing_an_inbox_is_audited_without_claiming_the_worker_read_it() {
    let home = tempfile::tempdir().expect("mail home");
    let (sender, receiver) = peers(home.path());
    let (_, id) = sender
        .send("receiver", "unread work", None)
        .expect("send work");
    drop(receiver);
    let shown = answer(home.path(), home.path(), &["inbox", RECEIVER, "--clear"])
        .expect("clear inactive inbox");
    assert!(shown.contains("unread work"), "{shown}");
    assert!(
        read_inbox(home.path(), RECEIVER)
            .expect("cleared inbox")
            .is_empty()
    );
    let history = std::fs::read_to_string(files::audit_path(home.path(), RECEIVER))
        .expect("clear operation must remain auditable");
    let events: Vec<serde_json::Value> = history
        .lines()
        .map(|line| serde_json::from_str(line).expect("audit event"))
        .collect();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["kind"], "clear_attempted");
    assert_eq!(events[1]["kind"], "cleared");
    assert_eq!(events[0]["mail"]["id"], id);
    assert_eq!(events[0]["mail"]["text"], "unread work");
}

#[test]
fn a_failed_clear_audit_preserves_the_inbox() {
    let home = tempfile::tempdir().expect("mail home");
    let (sender, receiver) = peers(home.path());
    sender
        .send("receiver", "keep this work", None)
        .expect("send work");
    drop(receiver);
    std::fs::create_dir_all(files::audit_path(home.path(), RECEIVER))
        .expect("make the audit destination unwritable as a file");
    answer(home.path(), home.path(), &["inbox", RECEIVER, "--clear"]).unwrap_err();
    let retained = read_inbox(home.path(), RECEIVER).expect("retained inbox");
    assert_eq!(retained.len(), 1);
    assert_eq!(retained[0].text, "keep this work");
}

#[test]
fn clearing_a_corrupt_inbox_fails_without_deleting_evidence() {
    let home = tempfile::tempdir().expect("mail home");
    let path = home.path().join("inbox").join(format!("{RECEIVER}.jsonl"));
    std::fs::create_dir_all(path.parent().expect("inbox folder")).expect("create inbox");
    let evidence = b"not a complete mail record\n";
    std::fs::write(&path, evidence).expect("corrupt inbox fixture");
    answer(home.path(), home.path(), &["inbox", RECEIVER, "--clear"]).unwrap_err();
    assert_eq!(
        std::fs::read(&path).expect("retained corrupt evidence"),
        evidence
    );
}
