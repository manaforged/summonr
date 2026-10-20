use std::process::{Command, Stdio};

const ID: &str = "000000000000000018d2297ad34281e1";

#[test]
fn profile_all_counts_the_sessions_it_read() {
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join("sessions").join(ID);
    std::fs::create_dir_all(&dir).unwrap();
    let header = format!(
        r#"{{"kind":"session","id":"{ID}","model":"glm","system":"","workspace":"/work"}}"#
    );
    let turn = r#"{"kind":"assistant","text":"ok","model":"glm","usage":{"input":10,"output":1,"cached":0,"millis":1},"at":3600000}"#;
    std::fs::write(dir.join("session.jsonl"), format!("{header}\n{turn}\n")).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_summonr"))
        .args(["profile", "--all"])
        .current_dir(home.path())
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env("SUMMONR_HOME", home.path())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("1 sessions (0 skipped unreadable)"), "{text}");
}
