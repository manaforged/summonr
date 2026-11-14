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
    let workspace = tempfile::tempdir().expect("temp dir");
    std::fs::write(workspace.path().join("AGENTS.md"), "AGENTS RULE").expect("write fixture");
    std::fs::create_dir(workspace.path().join("docs")).expect("create fixture dir");
    std::fs::write(workspace.path().join("docs/map.md"), "MAP RULE").expect("write fixture");
    std::fs::write(
        workspace.path().join("CLAUDE.md"),
        "@AGENTS.md\n@docs/map.md\n\nCLAUDE RULE",
    )
    .expect("write fixture");
    let prompt = trusted(workspace.path());
    assert!(prompt.contains("MAP RULE"), "{prompt}");
    assert!(prompt.contains("CLAUDE RULE"), "{prompt}");
    assert_eq!(prompt.matches("AGENTS RULE").count(), 1, "{prompt}");
}

#[test]
fn a_missing_at_include_stays_a_literal_line() {
    let workspace = tempfile::tempdir().expect("temp dir");
    std::fs::write(workspace.path().join("CLAUDE.md"), "@NOPE.md").expect("write fixture");
    let prompt = trusted(workspace.path());
    assert!(prompt.contains("@NOPE.md"), "{prompt}");
}

#[test]
fn at_include_cycles_terminate() {
    let workspace = tempfile::tempdir().expect("temp dir");
    std::fs::write(workspace.path().join("AGENTS.md"), "@CLAUDE.md").expect("write fixture");
    std::fs::write(workspace.path().join("CLAUDE.md"), "@AGENTS.md").expect("write fixture");
    let prompt = trusted(workspace.path());
    assert!(
        prompt.contains("@CLAUDE.md") || prompt.contains("@AGENTS.md"),
        "{prompt}"
    );
}

#[test]
fn a_workspace_system_file_keeps_the_preamble() {
    let workspace = tempfile::tempdir().expect("temp dir");
    std::fs::write(workspace.path().join("SYSTEM.md"), "WORKSPACE RULE").expect("write fixture");
    let prompt = trusted(workspace.path());
    assert!(
        prompt.contains("You are an expert coding assistant running inside summonr"),
        "{prompt}"
    );
    assert!(prompt.contains("WORKSPACE RULE"), "{prompt}");
}

#[test]
fn a_session_without_memory_is_told_nothing_persists() {
    let work = tempfile::tempdir().expect("temp dir");
    let home = tempfile::tempdir().expect("temp dir");
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
    let home = summonr_core::dirs::summonr_home().expect("summonr home");
    assert!(home.starts_with(std::env::temp_dir()), "{}", home.display());
}

fn context(workspace: &Path, home: &Path) -> String {
    let mut parts = Vec::new();
    instructions(&mut parts, workspace, Some(home));
    parts.join("\n\n")
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().expect("path has a parent")).expect("create fixture dir");
    std::fs::write(path, text).expect("write fixture");
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
    let root = tempfile::tempdir().expect("temp dir");
    let home = tempfile::tempdir().expect("temp dir");
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
    let workspace = tempfile::tempdir().expect("temp dir");
    let home = tempfile::tempdir().expect("temp dir");
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
    let workspace = tempfile::tempdir().expect("temp dir");
    let home = tempfile::tempdir().expect("temp dir");
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
    let workspace = tempfile::tempdir().expect("temp dir");
    let home = tempfile::tempdir().expect("temp dir");
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
    let workspace = tempfile::tempdir().expect("temp dir");
    let home = tempfile::tempdir().expect("temp dir");
    let dir = workspace.path();
    write(&dir.join("AGENTS.md"), "AGENTS RULE");
    std::os::unix::fs::symlink(dir.join("AGENTS.md"), dir.join("CLAUDE.md")).expect("symlink");
    write(&dir.join(".claude/CLAUDE.md"), "@../AGENTS.md\nDOT RULE");
    let prompt = context(dir, home.path());
    assert_eq!(prompt.matches("AGENTS RULE").count(), 1, "{prompt}");
    assert!(prompt.contains("DOT RULE"), "{prompt}");
}

fn custom(keep_coding: bool) -> Style {
    Style {
        name: "Diagrams first".to_string(),
        description: String::new(),
        instructions: "Start with a Mermaid diagram.".to_string(),
        keep_coding,
        force: false,
        origin: crate::plugins::agents::Origin::User,
    }
}

fn plain(workspace: &Path) -> String {
    styled(workspace, None, Sources::default(), None)
}

#[test]
fn the_default_style_keeps_the_whole_preamble() {
    let workspace = tempfile::tempdir().expect("temp dir");
    let prompt = plain(workspace.path());
    assert!(prompt.starts_with(&preamble(true)), "{prompt}");
    assert!(prompt.contains(IDENTITY) && prompt.contains(CODING_RULES));
    assert!(!prompt.contains(STYLE_OPEN), "{prompt}");
    assert_eq!(style_name(&prompt), None);
}

#[test]
fn a_custom_style_drops_the_coding_rules_and_sits_before_the_cwd() {
    let workspace = tempfile::tempdir().expect("temp dir");
    let prompt = styled(
        workspace.path(),
        None,
        Sources::default(),
        Some(&custom(false)),
    );
    assert!(!prompt.contains(IDENTITY), "{prompt}");
    assert!(!prompt.contains(CODING_RULES), "{prompt}");
    assert!(prompt.contains(TOOL_RULES), "{prompt}");
    order(
        &prompt,
        &[
            "<rules>",
            "Start with a Mermaid diagram.",
            "</output_style>",
            "<cwd>",
        ],
    );
    assert_eq!(style_name(&prompt), Some("Diagrams first"));
}

#[test]
fn keep_coding_instructions_keeps_the_coding_rules_beside_the_style() {
    let workspace = tempfile::tempdir().expect("temp dir");
    let prompt = styled(
        workspace.path(),
        None,
        Sources::default(),
        Some(&custom(true)),
    );
    assert!(prompt.starts_with(&preamble(true)), "{prompt}");
    assert!(prompt.contains("Start with a Mermaid diagram."), "{prompt}");
}

#[test]
fn restyling_round_trips_to_the_plain_prompt() {
    let workspace = tempfile::tempdir().expect("temp dir");
    let base = plain(workspace.path());
    let styled_once = restyle(&base, Some(&custom(false)));
    assert_eq!(restyle(&styled_once, Some(&custom(false))), styled_once);
    assert_eq!(restyle(&styled_once, None), base);
    assert_eq!(restyle("no cwd here", Some(&custom(false))), "no cwd here");
}

#[test]
fn a_replaced_preamble_keeps_its_text_under_a_style() {
    let work = tempfile::tempdir().expect("temp dir");
    let home = tempfile::tempdir().expect("temp dir");
    std::fs::write(home.path().join("SYSTEM.md"), "OWN PREAMBLE").expect("write fixture");
    let prompt = styled(
        work.path(),
        Some(home.path()),
        Sources::default(),
        Some(&custom(false)),
    );
    assert!(prompt.starts_with("OWN PREAMBLE"), "{prompt}");
    assert!(prompt.contains("Start with a Mermaid diagram."), "{prompt}");
}

#[test]
fn a_switch_note_names_the_style_or_the_default() {
    let note = style_note(&Pick::Custom(custom(false)));
    assert_eq!(
        note,
        "<output_style name=\"Diagrams first\">\nStart with a Mermaid diagram.\n</output_style>"
    );
    assert!(style_note(&Pick::Default).contains(DEFAULT_NOTE));
}
