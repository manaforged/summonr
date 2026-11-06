use crate::durable::log;
use crate::session::SessionInfo;

#[path = "audit_tests.rs"]
mod audit;

const ID: &str = "000000000000000018d2297ad34281e1";

fn info(home: &std::path::Path) -> SessionInfo {
    let path = log::sessions_dir(home).join(ID).join("session.jsonl");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "{\"kind\":\"user\",\"text\":\"Hello World\"}\n").unwrap();
    SessionInfo {
        id: ID.to_string(),
        path,
        cwd: None,
        model: None,
        preview: String::new(),
        name: None,
        parent: None,
        depth: 0,
        interactive: true,
        modified: None,
    }
}

#[test]
fn a_session_may_contain_only_words_its_journal_holds() {
    let home = tempfile::tempdir().unwrap();
    let info = info(home.path());
    assert!(info.may_contain("hello"));
    assert!(!info.may_contain("zebra"));
    let body = r#"{"kind":"assistant","at":42,"text":"answer","reasoning":"provider summary","vendor_field":{"kept":true}}"#;
    std::fs::write(&info.path, format!("{body}\n{{\"partial\":")).expect("audit fixture");
    let mut output = Vec::new();
    super::export_json(&info.path, &mut output).expect("structured audit export");
    let exported = std::str::from_utf8(&output).expect("audit UTF-8");
    assert_eq!(exported.lines().count(), 2);
    let record: serde_json::Value =
        serde_json::from_str(exported.lines().next().expect("record")).expect("audit JSON");
    assert_eq!(record["record"]["reasoning"], "provider summary");
    assert_eq!(record["record"]["vendor_field"]["kept"], true);
    assert_eq!(record["record"]["at"], 42);
}

#[test]
fn a_needle_the_raw_journal_cannot_rule_out_always_may_match() {
    let home = tempfile::tempdir().unwrap();
    let info = info(home.path());
    assert!(info.may_contain(""));
    assert!(info.may_contain("ze-bra"));
    assert!(info.may_contain("interrupted"));
}

#[test]
fn a_session_may_contain_a_word_from_its_name() {
    let home = tempfile::tempdir().unwrap();
    let info = info(home.path());
    assert!(!info.may_contain("leakhunt"));
    crate::session::set_name(&info.path, "Leakhunt run").unwrap();
    assert!(info.may_contain("leakhunt"));
}
