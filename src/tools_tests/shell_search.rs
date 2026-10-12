use super::*;

#[test]
fn the_search_commands_models_type_are_answered_by_search() {
    let (dir, tools) = workspace(&[("notes.txt", "needle in notes\nNeedle again\n")]);
    std::fs::create_dir_all(dir.path().join("Library/cache")).unwrap();
    std::fs::create_dir_all(dir.path().join("docs/old")).unwrap();
    std::fs::write(dir.path().join("Library/cache/refract-cache.txt"), "x").unwrap();
    std::fs::write(
        dir.path().join("docs/old/refract-notes.md"),
        "refract here\n",
    )
    .unwrap();
    let root = dir.path().display().to_string().replace('\\', "/");
    let logged = format!(
        "grep -ril refract {root}/docs 2>/dev/null | head; find {root} -maxdepth 3 -iname '*refract*' -not -path '*/Library/*' 2>/dev/null | head"
    );
    let answered = outcome(&tools, &logged);
    assert!(answered.starts_with("[`grep -ril refract"), "{answered}");
    assert!(answered.contains("answered by search"), "{answered}");
    assert!(answered.contains("docs/old/refract-notes.md"), "{answered}");
    assert!(!answered.contains("refract-cache.txt"), "{answered}");
    let exact = outcome(&tools, "grep -rn needle .");
    assert!(exact.contains("notes.txt:1:1:needle in notes"), "{exact}");
    assert!(!exact.contains("Needle again"), "{exact}");
    let listed = outcome(&tools, "rg -l Needle -i");
    assert!(
        listed.contains("notes.txt") && listed.contains("answered by search"),
        "{listed}"
    );
    let spotlight = outcome(&tools, &format!("mdfind -name refract -onlyin {root}"));
    assert!(spotlight.contains("refract-notes.md"), "{spotlight}");
    let exec = outcome(&tools, "find . -name '*.txt' -exec wc -l {} +");
    assert!(!exec.contains("answered by search"), "{exec}");
    let sorted = outcome(&tools, "grep -rn needle . | sort");
    assert!(!sorted.contains("answered by search"), "{sorted}");
    if cfg!(unix) {
        assert!(exec.contains("notes.txt"), "{exec}");
        assert!(sorted.contains("notes.txt"), "{sorted}");
    }
}
