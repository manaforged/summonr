use sigil::reactive::Runtime;
use tokio::sync::mpsc::unbounded_channel;

use super::super::app::App;
use super::{State, busy_info, frame_with};
use crate::tui::{Cmd, Msg};

fn peer() -> crate::mail::Agent {
    crate::mail::Agent {
        id: "0123456789abcdef0123456789abcdef".to_string(),
        name: "summonr-2".to_string(),
        kind: "summonr".to_string(),
        model: "stub-model".to_string(),
        cwd: "/work/summonr".to_string(),
        pid: 1,
        status: "idle".to_string(),
        last_seen: 0,
    }
}

#[test]
fn typing_at_offers_peer_sessions_by_name() {
    let lines = frame_with(busy_info(), 100, 24, State::Idle, |app| {
        app.update(Msg::Peers(vec![peer()]));
        app.replace("@summ".to_string());
    });
    assert!(
        lines.iter().any(|line| line.contains("summonr-2")),
        "{}",
        lines.join("\n")
    );
}

#[test]
fn a_prompt_that_names_a_peer_is_sent_to_it() {
    Runtime::scope("mention-send", || {
        let workspace = tempfile::tempdir().unwrap();
        std::fs::create_dir(workspace.path().join(".git")).unwrap();
        std::fs::write(workspace.path().join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        let (commands, mut sent) = unbounded_channel();
        let app = App::new(busy_info(), workspace.path().to_owned(), commands);
        app.update(Msg::Peers(vec![peer()]));
        app.replace("@summonr-2, the schema landed".to_string());
        app.submit();
        let mut routed = None;
        while let Ok(command) = sent.try_recv() {
            if let Cmd::Send(to, text) = command {
                routed = Some((to, text));
            }
        }
        assert_eq!(
            routed,
            Some((
                vec!["summonr-2".to_string()],
                "@summonr-2, the schema landed".to_string()
            ))
        );
    });
}
