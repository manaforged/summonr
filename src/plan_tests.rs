use super::*;

#[test]
fn a_short_approval_accepts_the_plan_and_a_request_does_not() {
    for reply in ["go", " Go ahead ", "YES", "do it"] {
        assert!(approves(reply), "{reply}");
    }
    for reply in ["go and also add tests", "no", ""] {
        assert!(!approves(reply), "{reply}");
    }
}

#[test]
fn the_plan_lives_beside_the_session_log_not_in_the_workspace() {
    let session = Path::new("/home/me/.summonr/sessions/abc/session.jsonl");
    assert!(file(session).starts_with("/home/me/.summonr/sessions/abc"));
}
