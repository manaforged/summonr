use std::path::PathBuf;

use super::child::{Seen, argv, outcome};
use super::store::{State, code_from};
use super::telegram::split;
use super::turn::Options;
use super::webhook::{Status, decide};
use super::{Action, addressed, route};

const OWNER: i64 = 7;
const STRANGER: i64 = 9;

fn paired() -> State {
    let mut state = State::default();
    state.offer(OWNER, OWNER, "ABC234", 0);
    assert!(state.pair("abc234", 10).is_some());
    state
}

#[test]
fn a_stranger_gets_one_pairing_code_then_silence() {
    let mut state = State::default();
    assert_eq!(route(&state, STRANGER, "hi", 0), Action::Pair);
    state.offer(STRANGER, STRANGER, "XYZ789", 0);
    assert_eq!(route(&state, STRANGER, "hi again", 5), Action::Drop);
    let expired = i64::try_from(crate::limits::limits().gateway_pair_secs).unwrap() + 1;
    assert_eq!(route(&state, STRANGER, "later", expired), Action::Pair);
}

#[test]
fn a_pairing_code_expires_and_works_once() {
    let mut state = State::default();
    state.offer(STRANGER, STRANGER, "XYZ789", 0);
    let late = i64::try_from(crate::limits::limits().gateway_pair_secs).unwrap() + 1;
    assert!(state.pair("XYZ789", late).is_none());
    assert!(!state.paired(STRANGER));
    state.offer(STRANGER, STRANGER, "XYZ789", 0);
    assert!(state.pair("XYZ789", 1).is_some());
    assert!(state.pair("XYZ789", 2).is_none());
    assert!(state.paired(STRANGER));
}

#[test]
fn a_paired_owner_runs_text_and_chat_commands() {
    let state = paired();
    assert_eq!(
        route(&state, OWNER, "fix the tests", 20),
        Action::Run("fix the tests".to_string())
    );
    assert_eq!(route(&state, OWNER, "/new", 20), Action::New);
    assert_eq!(route(&state, OWNER, "/stop@my_bot", 20), Action::Stop);
    assert_eq!(route(&state, OWNER, "/status", 20), Action::Status);
    assert_eq!(route(&state, OWNER, "   ", 20), Action::Drop);
    assert_eq!(route(&state, OWNER, "/start", 20), Action::Help);
    assert_eq!(route(&state, OWNER, "/help@my_bot", 20), Action::Help);
    assert_eq!(route(&state, STRANGER, "/new", 20), Action::Pair);
}

#[test]
fn pairing_codes_avoid_lookalike_characters() {
    let code = code_from(&[0, 1, 2, 3, 4, 5]).unwrap();
    assert_eq!(code.len(), 6);
    assert!(!code.contains(['0', 'O', '1', 'I', 'L']), "{code}");
}

#[test]
fn long_replies_split_at_line_breaks_under_the_limit() {
    let text = format!("{}\n{}\n{}", "a".repeat(6), "b".repeat(6), "c".repeat(20));
    assert_eq!(
        split(&text, 10),
        ["aaaaaa", "bbbbbb", "cccccccccc", "cccccccccc"]
    );
    assert_eq!(split("short", 4096), ["short"]);
}

#[test]
fn a_chat_turn_resumes_its_session_and_keeps_text_out_of_flags() {
    let options = Options {
        endpoint: Some("zai".to_string()),
        model: None,
        dir: PathBuf::from("/work"),
    };
    assert_eq!(
        argv(&options, Some("s1"), "--help me"),
        ["zai", "--session", "s1", "--json", "-p", "--", "--help me"]
    );
}

#[test]
fn the_turn_reply_is_the_last_answer_with_any_abort() {
    let mut seen = Seen::default();
    seen.read(r#"{"type":"session","id":"s9"}"#);
    seen.read(r#"{"type":"assistant","text":"first"}"#);
    seen.read(r#"{"type":"assistant","text":"done"}"#);
    seen.read(r#"{"type":"aborted","code":"time_budget","reason":"out of time"}"#);
    assert_eq!(seen.session.as_deref(), Some("s9"));
    assert_eq!(outcome(&seen, Some(0), &[]), "done\n\naborted: out of time");
}

#[test]
fn the_webhook_fires_only_with_the_job_token() {
    let token = |name: &str| (name == "nightly").then(|| "t0ken".to_string());
    assert_eq!(
        decide("POST", "/run/nightly", Some("Bearer t0ken"), token),
        Ok("nightly".to_string())
    );
    assert_eq!(
        decide("POST", "/run/nightly", Some("bearer t0ken"), token),
        Ok("nightly".to_string())
    );
    assert_eq!(
        decide("POST", "/run/nightly", Some("Bearer wrong"), token),
        Err(Status::Unauthorized)
    );
    assert_eq!(
        decide("POST", "/run/other", Some("Bearer t0ken"), token),
        Err(Status::Unauthorized)
    );
    assert_eq!(
        decide("POST", "/run/nightly", None, token),
        Err(Status::Unauthorized)
    );
    assert_eq!(
        decide("GET", "/run/nightly", Some("Bearer t0ken"), token),
        Err(Status::WrongMethod)
    );
    assert_eq!(
        decide("POST", "/admin", Some("Bearer t0ken"), token),
        Err(Status::NotFound)
    );
}

#[test]
fn commands_for_another_bot_are_ignored() {
    assert!(addressed("/stop", Some("summonr_bot")));
    assert!(addressed("/stop@Summonr_Bot now", Some("summonr_bot")));
    assert!(!addressed("/stop@other_bot", Some("summonr_bot")));
    assert!(!addressed("/new@other_bot", None));
    assert!(addressed("fix the tests", None));
}

#[test]
fn unpairing_revokes_chats_and_notifications() {
    let mut state = paired();
    state.set_session(OWNER, Some("s1".to_string()));
    state.set_session(-100, Some("group".to_string()));
    assert_eq!(state.paired_chats(), [OWNER]);
    assert!(state.unpair(OWNER));
    assert!(state.session(OWNER).is_none());
    assert!(state.paired_chats().is_empty());
}

#[test]
fn the_reply_says_when_the_check_failed_or_the_run_broke() {
    let mut seen = Seen::default();
    seen.read(r#"{"type":"assistant","text":"done"}"#);
    assert_eq!(outcome(&seen, Some(0), &[]), "done");
    assert!(outcome(&seen, Some(2), &[]).contains("the check failed"));
    let silent = Seen::default();
    let broke = outcome(&silent, Some(1), &["summonr: no provider".to_string()]);
    assert!(broke.contains("failed (exit 1)"), "{broke}");
    assert!(broke.contains("no provider"), "{broke}");
}

#[test]
fn the_poll_offset_survives_a_restart() {
    let state = State {
        offset: 42,
        ..State::default()
    };
    let text = toml::to_string(&state).unwrap();
    assert_eq!(toml::from_str::<State>(&text).unwrap().offset, 42);
}
