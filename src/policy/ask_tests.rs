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

#[test]
fn toggling_approval_keeps_destructive_checks_ask_patterns_and_denies() {
    let mut policy = layered(&[r#"ask = ["git push"]
deny = ["forbidden"]"#]);
    let workspace = Path::new("/app");
    assert_eq!(
        policy.needs_approval("git reset --hard", workspace, workspace),
        None
    );
    assert!(policy.toggle_approval());
    assert!(
        policy
            .needs_approval("git reset --hard", workspace, workspace)
            .is_some()
    );
    assert_eq!(
        policy.needs_approval("git status", workspace, workspace),
        None
    );
    assert!(!policy.toggle_approval());
    assert_eq!(
        policy.needs_approval("git reset --hard", workspace, workspace),
        None
    );
    assert!(
        policy
            .needs_approval("rm -rf /", workspace, workspace)
            .is_some()
    );
    assert!(
        policy
            .needs_approval("git push origin main", workspace, workspace)
            .is_some()
    );
    let error = policy
        .command("forbidden", workspace, workspace)
        .expect_err("configured deny remains effective");
    assert!(
        error.to_string().contains("denied by policy `forbidden`"),
        "{error}"
    );
    assert!(!policy.strict);
}

#[test]
fn configured_confirmation_and_strict_read_only_policy_cannot_be_lowered() {
    let mut policy = layered(&[r#"confirm = "risky"
strict = true
allow = ["git"]
write = ["src/**"]"#]);
    policy.set_read_only(true);
    assert!(policy.is_approval());
    assert!(policy.toggle_approval());
    assert!(policy.toggle_approval());
    let workspace = Path::new("/app");
    assert!(
        policy
            .needs_approval("git reset --hard", workspace, workspace)
            .is_some()
    );
    let error = policy
        .command("git reset --hard", workspace, workspace)
        .expect_err("strict mode still refuses risky commands");
    assert!(
        error.to_string().contains("strict mode never runs it"),
        "{error}"
    );
    assert!(policy.strict && policy.is_read_only());
    assert_eq!(policy.write, [vec!["src/**".to_owned()]]);
    let error = policy
        .write_globs("outside.rs")
        .expect_err("write restrictions remain effective");
    assert!(
        error
            .to_string()
            .contains("not permitted by the policy write globs"),
        "{error}"
    );
    let error = policy
        .write_path("src/main.rs")
        .expect_err("read-only mode remains effective");
    assert!(
        error.to_string().contains("this session is read-only"),
        "{error}"
    );
}
