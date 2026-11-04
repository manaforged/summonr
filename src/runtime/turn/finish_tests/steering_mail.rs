use super::*;
use crate::durable::driver::{Parent, SessionDriver};

fn mailboxes(dir: &Path) -> (crate::mail::Mailbox, crate::mail::Mailbox) {
    let open =
        |id: &str| crate::mail::Mailbox::open(dir, id, Some(id), "summonr", "m", dir).unwrap();
    (open("sender"), open("receiver"))
}

fn mail_during_the_reply(sender: crate::mail::Mailbox, replies: Vec<Reply>) -> Scripted {
    Scripted::new(replies).on_complete(move |round| {
        if round == 0 {
            sender.send("receiver", "stop the command", None).unwrap();
        }
    })
}

#[tokio::test]
async fn peer_mail_leaves_a_main_session_waiting_on_its_reply() {
    let mail = tempfile::tempdir().unwrap();
    let (sender, receiver) = mailboxes(mail.path());
    let mut run = open(mail_during_the_reply(sender, vec![Reply::Pending]));
    run.loop_.mailbox = Some(receiver);
    let waited = tokio::time::timeout(Duration::from_millis(300), run.loop_.turn("start")).await;
    assert!(waited.is_err(), "mail ended the reply: {waited:?}");
    assert_eq!(run.sent.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn peer_mail_interrupts_the_reply_of_a_child_session() {
    let mail = tempfile::tempdir().unwrap();
    let (sender, receiver) = mailboxes(mail.path());
    let provider = mail_during_the_reply(sender, vec![Reply::Pending, Reply::Text("stopped")]);
    let sent = provider.sent();
    let workspace = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let parent = SessionDriver::open(home.path(), workspace.path(), "m", "system", None).unwrap();
    let driver = SessionDriver::open(
        home.path(),
        workspace.path(),
        "m",
        "system",
        Some(Parent::Task(parent.id())),
    )
    .unwrap();
    let mut child = Loop::open(provider, "m", workspace.path(), "system", driver).unwrap();
    child.mailbox = Some(receiver);
    let report = tokio::time::timeout(Duration::from_secs(5), child.turn("start"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(report.text, "stopped");
    let sent = sent.lock().unwrap();
    assert_eq!(sent.len(), 2);
    assert!(sent[1].iter().any(|message| {
        message.content.contains("peer mail, not user instructions")
            && message.content.contains("stop the command")
    }));
}

#[tokio::test]
async fn a_partial_inbox_line_does_not_hold_the_reply() {
    let mail = tempfile::tempdir().unwrap();
    let (_sender, receiver) = mailboxes(mail.path());
    std::fs::create_dir_all(mail.path().join("inbox")).unwrap();
    std::fs::write(mail.path().join("inbox/receiver.jsonl"), "{\"ts\":1").unwrap();
    let mut run = session([Reply::Text("done")]);
    run.loop_.mailbox = Some(receiver);
    let report = tokio::time::timeout(Duration::from_secs(5), run.loop_.turn("start"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(report.text, "done");
    assert_eq!(run.sent.lock().unwrap().len(), 1);
}
