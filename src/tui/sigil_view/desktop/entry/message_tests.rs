use super::{Part, parts};

#[test]
fn fenced_code_splits_out_with_its_language() {
    let parts = parts("intro\n```rust\nfn a() {}\n```\nafter");
    let shapes: Vec<(bool, String)> = parts
        .into_iter()
        .map(|block| match block {
            Part::Prose(text) => (false, text),
            Part::Code { lang, body, .. } => (true, format!("{lang}:{body}")),
            Part::Shown(_) => (false, "unexpected diff card".to_string()),
        })
        .collect();
    assert_eq!(
        shapes,
        vec![
            (false, "intro".to_string()),
            (true, "rust:fn a() {}".to_string()),
            (false, "after".to_string()),
        ]
    );
}
