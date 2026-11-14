use super::*;

fn trusted(workspace: &Path) -> String {
    compose(
        workspace,
        None,
        Sources {
            trust: true,
            ..Sources::default()
        },
    )
}

#[test]
fn an_at_include_pulls_the_referenced_file_in_once() {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::write(workspace.path().join("AGENTS.md"), "AGENTS RULE").unwrap();
    std::fs::create_dir(workspace.path().join("docs")).unwrap();
    std::fs::write(workspace.path().join("docs/map.md"), "MAP RULE").unwrap();
    std::fs::write(
        workspace.path().join("CLAUDE.md"),
        "@AGENTS.md\n@docs/map.md\n\nCLAUDE RULE",
    )
    .unwrap();
    let prompt = trusted(workspace.path());
    assert!(prompt.contains("MAP RULE"), "{prompt}");
    assert!(prompt.contains("CLAUDE RULE"), "{prompt}");
    assert_eq!(prompt.matches("AGENTS RULE").count(), 1, "{prompt}");
}

#[test]
fn a_missing_at_include_stays_a_literal_line() {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::write(workspace.path().join("CLAUDE.md"), "@NOPE.md").unwrap();
    let prompt = trusted(workspace.path());
    assert!(prompt.contains("@NOPE.md"), "{prompt}");
}

#[test]
fn at_include_cycles_terminate() {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::write(workspace.path().join("AGENTS.md"), "@CLAUDE.md").unwrap();
    std::fs::write(workspace.path().join("CLAUDE.md"), "@AGENTS.md").unwrap();
    let prompt = trusted(workspace.path());
    assert!(
        prompt.contains("@CLAUDE.md") || prompt.contains("@AGENTS.md"),
        "{prompt}"
    );
}

#[test]
fn a_workspace_system_file_keeps_the_preamble() {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::write(workspace.path().join("SYSTEM.md"), "WORKSPACE RULE").unwrap();
    let prompt = trusted(workspace.path());
    assert!(
        prompt.contains("You are an expert coding assistant running inside summonr"),
        "{prompt}"
    );
    assert!(prompt.contains("WORKSPACE RULE"), "{prompt}");
}

#[test]
fn a_session_without_memory_is_told_nothing_persists() {
    let work = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let off = compose(
        work.path(),
        Some(home.path()),
        Sources {
            trust: true,
            ..Sources::default()
        },
    );
    assert!(off.contains(crate::memory::DISABLED), "{off}");
    let on = compose(
        work.path(),
        Some(home.path()),
        Sources {
            trust: true,
            memory: true,
            ..Sources::default()
        },
    );
    assert!(!on.contains(crate::memory::DISABLED), "{on}");
}

#[test]
fn tests_keep_summonr_state_out_of_the_real_home() {
    let home = summonr_core::dirs::summonr_home().unwrap();
    assert!(home.starts_with(std::env::temp_dir()), "{}", home.display());
}

fn context(workspace: &Path, home: &Path) -> String {
    let mut parts = Vec::new();
    instructions(&mut parts, workspace, Some(home));
    parts.join("\n\n")
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn order(prompt: &str, markers: &[&str]) {
    let at: Vec<usize> = markers
        .iter()
        .map(|marker| {
            prompt
                .find(marker)
                .unwrap_or_else(|| panic!("{marker}: {prompt}"))
        })
        .collect();
    assert!(at.is_sorted(), "{markers:?}: {prompt}");
}

#[test]
fn claude_files_load_from_parents_down_with_local_notes_last() {
    let root = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let workspace = root.path().join("repo");
    write(&root.path().join("CLAUDE.md"), "PARENT RULE");
    write(&root.path().join("CLAUDE.local.md"), "PARENT LOCAL");
    write(&workspace.join("AGENTS.md"), "AGENTS RULE");
    write(&workspace.join("CLAUDE.md"), "ROOT RULE");
    write(&workspace.join(".claude/CLAUDE.md"), "DOT RULE");
    write(&workspace.join("CLAUDE.local.md"), "LOCAL RULE");
    let prompt = context(&workspace, home.path());
    order(
        &prompt,
        &[
            "PARENT RULE",
            "PARENT LOCAL",
            "AGENTS RULE",
            "ROOT RULE",
            "DOT RULE",
            "LOCAL RULE",
        ],
    );
    assert!(prompt.contains("path=\".claude/CLAUDE.md\""), "{prompt}");
}

#[test]
fn rules_load_recursively_after_user_rules_and_skip_path_scoped_ones() {
    let workspace = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    write(&home.path().join(".claude/rules/me.md"), "USER RULE");
    write(&workspace.path().join(".claude/CLAUDE.md"), "DOT RULE");
    let rules = workspace.path().join(".claude/rules");
    write(&rules.join("a.md"), "---\nother: x\n---\nPLAIN RULE");
    write(&rules.join("sub/b.md"), "NESTED RULE");
    write(
        &rules.join("c.md"),
        "---\npaths:\n  - \"src/**/*.ts\"\n---\nLIST RULE",
    );
    write(
        &rules.join("d.md"),
        "---\npaths: \"src/*.rs, lib/*.rs\"\n---\nSTRING RULE",
    );
    write(&rules.join("e.txt"), "TEXT RULE");
    write(&workspace.path().join("CLAUDE.local.md"), "LOCAL RULE");
    let prompt = context(workspace.path(), home.path());
    order(
        &prompt,
        &[
            "USER RULE",
            "DOT RULE",
            "PLAIN RULE",
            "NESTED RULE",
            "LOCAL RULE",
        ],
    );
    for absent in ["LIST RULE", "STRING RULE", "TEXT RULE", "other: x"] {
        assert!(!prompt.contains(absent), "{absent}: {prompt}");
    }
}

#[test]
fn imports_expand_anywhere_outside_code_with_home_and_escaped_paths() {
    let workspace = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let dir = workspace.path();
    write(&dir.join("inline.md"), "INLINE RULE");
    write(&dir.join("Design Docs/api.md"), "SPACED RULE");
    write(&home.path().join(".claude/mine.md"), "HOME RULE");
    for name in ["span.md", "fence.md", "quoted.md", "mail.md"] {
        write(&dir.join(name), &format!("{name} LEAKED"));
    }
    write(
        &dir.join("CLAUDE.md"),
        "See @inline.md for more.\n- api @Design\\ Docs/api.md\n- @~/.claude/mine.md\nKeep `@span.md` literal.\n```\n@fence.md\n```\n@\"quoted.md\"\nmail me@mail.md",
    );
    let prompt = context(dir, home.path());
    for present in [
        "See @inline.md for more.",
        "INLINE RULE",
        "SPACED RULE",
        "HOME RULE",
    ] {
        assert!(prompt.contains(present), "{present}: {prompt}");
    }
    assert!(!prompt.contains("LEAKED"), "{prompt}");
}

#[test]
fn imports_stop_after_four_hops() {
    let workspace = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let dir = workspace.path();
    write(&dir.join("CLAUDE.md"), "@1.md");
    for hop in 1..=5 {
        write(
            &dir.join(format!("{hop}.md")),
            &format!("HOP {hop}\n@{}.md", hop + 1),
        );
    }
    let prompt = context(dir, home.path());
    assert!(prompt.contains("HOP 4"), "{prompt}");
    assert!(!prompt.contains("HOP 5"), "{prompt}");
}

#[cfg(unix)]
#[test]
fn a_claude_file_linked_to_agents_loads_it_once() {
    let workspace = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let dir = workspace.path();
    write(&dir.join("AGENTS.md"), "AGENTS RULE");
    std::os::unix::fs::symlink(dir.join("AGENTS.md"), dir.join("CLAUDE.md")).unwrap();
    write(&dir.join(".claude/CLAUDE.md"), "@../AGENTS.md\nDOT RULE");
    let prompt = context(dir, home.path());
    assert_eq!(prompt.matches("AGENTS RULE").count(), 1, "{prompt}");
    assert!(prompt.contains("DOT RULE"), "{prompt}");
}
