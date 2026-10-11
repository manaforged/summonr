use super::*;

fn one(text: &str) -> String {
    let home = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let path = home.path().join(DIR).join(FILE);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, text).unwrap();
    sections(&files(home.path(), work.path()), work.path()).join("\n")
}

#[test]
fn an_oversized_memory_is_cut_with_a_marker() {
    let cap = crate::limits::limits().memory_bytes;
    let text = one(&"é".repeat(cap));
    assert!(text.contains(&format!(
        "[memory truncated at {cap} bytes; prune this file]"
    )));
    assert!(text.len() < cap + 200);
}

#[test]
fn hidden_characters_and_secrets_never_reach_the_prompt() {
    let text = one(
        "2026-10-11 deploy\u{200B}key\u{202E} is set\nAPI_TOKEN=placeholder-value-placeholder\n",
    );
    assert!(text.contains("2026-10-11 deploykey is set"), "{text}");
    assert!(!text.contains("placeholder-value"), "{text}");
    assert!(text.contains("API_TOKEN=[secret]"), "{text}");
}

#[test]
fn an_empty_memory_adds_no_section() {
    assert_eq!(one("  \n"), "");
}
