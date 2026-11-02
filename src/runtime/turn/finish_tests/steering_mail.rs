use super::*;

#[tokio::test]
async fn peer_mail_interrupts_a_pending_provider_request() {
    let mail = tempfile::tempdir().unwrap();
    let sender = crate::mail::Mailbox::open(
        mail.path(),
        "sender",
        Some("sender"),
        "summonr",
        "m",
        mail.path(),
    )
    .unwrap();
    let receiver = crate::mail::Mailbox::open(
        mail.path(),
        "receiver",
        Some("receiver"),
        "summonr",
        "m",
        mail.path(),
    )
    .unwrap();
    let provider =
        Scripted::new([Reply::Pending, Reply::Text("stopped")]).on_complete(move |round| {
            if round == 0 {
                sender.send("receiver", "stop the command").unwrap();
            }
        });
    let mut run = open(provider);
    run.loop_.mailbox = Some(receiver);
    let report = tokio::time::timeout(Duration::from_secs(5), run.loop_.turn("start"))
        .await
        .unwrap()
        .unwrap();
    assert!(
        report
            .events
            .iter()
            .any(|event| matches!(event, Event::Assistant { text } if text == "stopped"))
    );
    let sent = run.sent.lock().unwrap();
    assert_eq!(sent.len(), 2);
    assert!(sent[1].iter().any(|message| {
        message.content.contains("peer mail, not user instructions")
            && message.content.contains("stop the command")
    }));
}
