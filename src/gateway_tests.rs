use std::path::PathBuf;

use super::store::{State, code_from};
use super::telegram::split;
use super::turn::{Options, Seen, argv};
use super::webhook::{Reply, decide};
use super::{Action, route};

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
    assert_eq!(route(&state, STRANGER, STRANGER, "hi", 0), Action::Pair);
    state.offer(STRANGER, STRANGER, "XYZ789", 0);
    assert_eq!(
        route(&state, STRANGER, STRANGER, "hi again", 5),
        Action::Drop
    );
    let expired = i64::try_from(crate::limits::limits().gateway_pair_secs).unwrap() + 1;
    assert_eq!(
        route(&state, STRANGER, STRANGER, "later", expired),
        Action::Pair
    );
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
        route(&state, OWNER, OWNER, "fix the tests", 20),
        Action::Run("fix the tests".to_string())
    );
    assert_eq!(route(&state, OWNER, OWNER, "/new", 20), Action::New);
    assert_eq!(
        route(&state, OWNER, OWNER, "/stop@my_bot", 20),
        Action::Stop
    );
    assert_eq!(route(&state, OWNER, OWNER, "/status", 20), Action::Status);
    assert_eq!(route(&state, OWNER, OWNER, "   ", 20), Action::Drop);
    assert_eq!(route(&state, STRANGER, OWNER, "/new", 20), Action::Pair);
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
    assert_eq!(seen.reply(), "done\n\naborted: out of time");
}

#[test]
fn the_webhook_fires_only_with_the_job_token() {
    let token = |name: &str| (name == "nightly").then(|| "t0ken".to_string());
    assert_eq!(
        decide("POST", "/run/nightly", Some("Bearer t0ken"), token),
        Reply::Fire("nightly".to_string())
    );
    assert_eq!(
        decide("POST", "/run/nightly", Some("Bearer wrong"), token),
        Reply::Unauthorized
    );
    assert_eq!(
        decide("POST", "/run/other", Some("Bearer t0ken"), token),
        Reply::Unauthorized
    );
    assert_eq!(
        decide("POST", "/run/nightly", None, token),
        Reply::Unauthorized
    );
    assert_eq!(
        decide("GET", "/run/nightly", Some("Bearer t0ken"), token),
        Reply::NotFound
    );
    assert_eq!(
        decide("POST", "/admin", Some("Bearer t0ken"), token),
        Reply::NotFound
    );
}
