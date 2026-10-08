use std::path::Path;

use super::super::{File, Policy};

fn layered(files: &[&str]) -> Policy {
    let mut policy = Policy::builtin();
    for text in files {
        policy.overlay(toml::from_str::<File>(text).unwrap());
    }
    policy
}

#[test]
fn an_ask_pattern_asks_before_a_push_and_leaves_other_commands_alone() {
    let policy = layered(&[r#"ask = ["git push"]"#]);
    let workspace = Path::new("/app");
    let reason = policy
        .needs_approval("git push origin main", workspace, workspace)
        .unwrap();
    assert!(reason.contains("ask pattern `git push`"), "{reason}");
    assert_eq!(
        policy.needs_approval("git status", workspace, workspace),
        None
    );
    assert_eq!(
        Policy::builtin().needs_approval("git push origin main", workspace, workspace),
        None
    );
}

#[test]
fn repository_ask_patterns_add_to_the_user_list() {
    let policy = layered(&[
        r#"ask = ["git push"]"#,
        r#"ask = ["cargo publish", "git push"]"#,
    ]);
    let workspace = Path::new("/app");
    assert_eq!(policy.ask, ["git push", "cargo publish"]);
    assert!(
        policy
            .needs_approval("cargo publish -p summonr", workspace, workspace)
            .is_some()
    );
}
