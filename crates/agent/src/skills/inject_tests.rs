use super::{Piece, pieces};

fn commands(text: &str) -> Vec<&str> {
    pieces(text)
        .into_iter()
        .filter_map(|piece| match piece {
            Piece::Shell { command, .. } => Some(command),
            Piece::Text(_) => None,
        })
        .collect()
}

#[test]
fn blocks_shown_inside_markdown_code_stay_text() {
    let text = "Run `` !`false` `` as shown.\n````markdown\n```!\nfalse\n```\n!`false`\n````\n~~~\n!`false`\n~~~\nNow !`echo hi` and\n```!\necho there\n```\n!`echo last`";
    assert_eq!(commands(text), ["echo hi", "echo there", "echo last"]);
    let written: String = pieces(text)
        .into_iter()
        .map(|piece| match piece {
            Piece::Text(text) | Piece::Shell { pattern: text, .. } => text,
        })
        .collect();
    assert_eq!(written, text);
}

#[test]
fn a_three_tick_example_around_a_block_runs_nothing() {
    let text = "```markdown\n```!\nfalse\n```\n```\nthen !`false`";
    assert!(commands(text).is_empty());
}

#[test]
fn a_loaded_skill_runs_only_its_live_blocks_and_hides_its_frontmatter() {
    let workspace = tempfile::tempdir().expect("workspace");
    let dir = workspace.path().join(".claude/skills/probe-doc");
    std::fs::create_dir_all(&dir).expect("skill directory");
    let file = "---\ndescription: !`false`\ncontext: fork\n---\nShown as `` !`false` ``.\n````markdown\n```!\nfalse\n```\n````\nLive: !`echo hi`\n";
    std::fs::write(dir.join("SKILL.md"), file).expect("skill file");
    let policy = crate::policy::Policy::builtin();
    let mut tools =
        crate::tools::ToolRegistry::with_policy_and_search(workspace.path(), policy, None)
            .expect("registry");
    tools.set_trust(true);
    let input = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("runtime")
        .block_on(tools.render_skill("probe-doc"))
        .expect("no `false` block runs");
    let text = input.text;
    assert!(text.starts_with("Shown as `` !`false` ``."), "{text}");
    assert!(!text.contains("context: fork"), "{text}");
    assert!(text.ends_with("Live: hi"), "{text}");
}

#[test]
fn keys_summonr_does_not_apply_are_named_and_inherit_is_not() {
    let front = summonr_core::text::Frontmatter::parse(
        "---\nmodel: inherit\neffort: high\nhooks:\n  Stop:\n    - command: true\n---\nbody",
    );
    let meta = super::super::Meta::parse(&front);
    assert_eq!(meta.unapplied, ["effort", "hooks"]);
    assert_eq!(
        meta.notes("skill", "probe"),
        [
            "skill probe effort (summonr does not apply it)",
            "skill probe hooks (summonr does not apply it)"
        ]
    );
}

#[test]
fn an_at_path_inside_code_is_not_attached() {
    let workspace = tempfile::tempdir().expect("workspace");
    std::fs::write(workspace.path().join(".env"), "probe env file").expect("env");
    std::fs::write(workspace.path().join("notes.txt"), "probe notes").expect("notes");
    let dir = workspace.path().join(".claude/skills/probe-at");
    std::fs::create_dir_all(&dir).expect("skill directory");
    let file = "Example:\n```sh\ncat @.env\n```\nNot `@.env` either.\nRead @notes.txt\n";
    std::fs::write(dir.join("SKILL.md"), file).expect("skill file");
    let policy = crate::policy::Policy::builtin();
    let tools = crate::tools::ToolRegistry::with_policy_and_search(workspace.path(), policy, None)
        .expect("registry");
    let input = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("runtime")
        .block_on(tools.render_skill("probe-at"))
        .expect("a code example does not attach .env");
    let text = input.text;
    assert!(text.contains("--- notes.txt ---\nprobe notes"), "{text}");
    assert!(!text.contains("--- .env ---"), "{text}");
}
