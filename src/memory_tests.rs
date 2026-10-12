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
        "2026-10-11 deploy\u{200B}key\u{202E}\u{061C}\u{00AD} is set\nAPI_TOKEN=placeholder-value-placeholder\n",
    );
    assert!(text.contains("2026-10-11 deploykey is set"), "{text}");
    assert!(!text.contains("placeholder-value"), "{text}");
    assert!(text.contains("API_TOKEN=[secret]"), "{text}");
}

#[test]
fn an_empty_memory_adds_no_section() {
    assert_eq!(one("  \n"), "");
}

#[test]
fn the_memory_report_uses_spaces_and_counts_entries() {
    let home = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let path = home.path().join(DIR).join(FILE);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        "# Memory\n- 2026-10-11 tabs\n2026-10-12 trunk-stable\n",
    )
    .unwrap();
    let text = report(&files(home.path(), work.path()));
    assert!(!text.contains('\t'), "{text}");
    assert!(text.starts_with("user memory      "), "{text}");
    assert!(text.contains("bytes, 2 entries"), "{text}");
    assert!(text.contains("project memory   "), "{text}");
    assert!(text.contains("\n  empty\n"), "{text}");
}

#[test]
fn look_alike_repository_paths_get_separate_memory() {
    let home = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let dash = root.path().join("a-b");
    let under = root.path().join("a_b");
    std::fs::create_dir_all(&dash).unwrap();
    std::fs::create_dir_all(&under).unwrap();
    let project = |dir: &Path| {
        files(home.path(), dir)
            .into_iter()
            .find(|file| file.scope == Scope::Project)
            .unwrap()
            .path
    };
    assert_ne!(project(&dash), project(&under));
    assert_eq!(project(&dash), project(&dash));
}
