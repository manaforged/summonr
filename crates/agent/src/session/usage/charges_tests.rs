use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::provider::Price;
use crate::session::{Spend, spend};

const ID: &str = "000000000000000018d2297ad34281e9";
const AT: &str = r#","at":3600000"#;

fn journal(home: &Path) -> PathBuf {
    let path = crate::durable::log::sessions_dir(home)
        .join(ID)
        .join("session.jsonl");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    path
}

fn head(model: &str) -> String {
    format!(
        r#"{{"kind":"session","id":"{ID}","model":"{model}","system":"","workspace":"/work/iota"}}"#
    ) + "\n"
}

fn turn(model: &str, usage: &str, at: &str) -> String {
    format!(r#"{{"kind":"assistant","text":"ok","model":"{model}","usage":{usage}{at}}}"#) + "\n"
}

fn usage(input: u64) -> String {
    format!(r#"{{"input":{input},"output":1,"cached":0,"millis":1}}"#)
}

fn total(rows: &[Spend]) -> u64 {
    rows.iter().map(|row| row.input).sum()
}

#[test]
fn a_priced_turn_charges_cache_writes_at_the_write_rate() {
    let home = tempfile::tempdir().unwrap();
    let written = r#"{"input":1000000,"output":0,"cached":0,"written":1000000,"millis":1}"#;
    std::fs::write(journal(home.path()), head("m") + &turn("m", written, AT)).unwrap();
    let price: Price = serde_json::from_value(
        serde_json::json!({"input": 1.0, "output": 1.0, "cache_write": 2.0}),
    )
    .unwrap();
    let prices = BTreeMap::from([("m".to_string(), price)]);
    let rows = spend(home.path(), &[], &prices).unwrap();
    let cost: f64 = rows.iter().filter_map(|row| row.cost).sum();
    assert!((cost - 2.0).abs() < 1e-9, "{cost}");
}

#[test]
fn a_grown_journal_reads_only_the_appended_turns() {
    let home = tempfile::tempdir().unwrap();
    let path = journal(home.path());
    let prices = BTreeMap::new();
    std::fs::write(&path, head("m") + &turn("m", &usage(120), AT)).unwrap();
    assert_eq!(total(&spend(home.path(), &[], &prices).unwrap()), 120);
    let rewritten = head("m") + &turn("m", &usage(990), AT) + &turn("m", &usage(30), AT);
    std::fs::write(&path, rewritten).unwrap();
    assert_eq!(total(&spend(home.path(), &[], &prices).unwrap()), 150);
}

#[test]
fn an_unnamed_turn_after_a_resume_bills_the_resumed_model() {
    let home = tempfile::tempdir().unwrap();
    let prompt = r#"{"kind":"user","text":"go","start":true}"#.to_string() + "\n";
    let body = head("glm") + &prompt + &head("qwen") + &turn("", &usage(10), AT);
    std::fs::write(journal(home.path()), body).unwrap();
    let rows = spend(home.path(), &[], &BTreeMap::new()).unwrap();
    let models: Vec<&str> = rows.iter().map(|row| row.model.as_str()).collect();
    assert_eq!(models, ["qwen"]);
}

#[test]
fn an_undated_turn_is_billed_to_the_journals_modified_hour() {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(journal(home.path()), head("m") + &turn("m", &usage(10), "")).unwrap();
    let rows = spend(home.path(), &[], &BTreeMap::new()).unwrap();
    assert_eq!(rows.len(), 1);
    assert_ne!(rows[0].hour, 0);
}
