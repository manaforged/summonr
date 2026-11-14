use super::*;
use crate::policy::Scope;
use summonr_types::catalog::Builtin;

fn agent(origin: Origin, name: &str) -> Agent {
    Agent {
        origin,
        name: name.to_string(),
        description: String::new(),
        tools: None,
        disallowed: None,
        model: None,
        color: None,
        skills: Vec::new(),
        instructions: String::new(),
    }
}

fn plugin_dir(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (path, text) in files {
        let path = dir.path().join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    dir
}

#[test]
fn a_block_list_of_tools_is_read_and_not_dropped() {
    let dir = plugin_dir(&[(
        "agents/reviewer.md",
        "---\nname: reviewer\ntools:\n  - Read\n  - Grep\n---\nReview it.\n",
    )]);
    let workspace = tempfile::tempdir().unwrap();
    let plugin = Plugin::at("demo".to_string(), dir.path().to_owned());
    let [found] = of_plugin(&plugin, workspace.path()).try_into().unwrap();
    assert_eq!(
        found.tools,
        Some(vec!["Read".to_string(), "Grep".to_string()])
    );
    assert_eq!(found.instructions, "Review it.");
}

#[test]
fn agents_that_share_a_name_are_listed_by_plugin() {
    let all = [
        agent(Origin::Plugin("alpha".to_string()), "reviewer"),
        agent(Origin::Plugin("beta".to_string()), "reviewer"),
        agent(Origin::Plugin("beta".to_string()), "writer"),
    ];
    assert_eq!(labels(&all), ["alpha:reviewer", "beta:reviewer", "writer"]);
}

#[test]
fn a_user_agent_keeps_its_bare_name_beside_a_plugin_agent() {
    let all = [
        agent(Origin::User, "reviewer"),
        agent(Origin::Plugin("beta".to_string()), "reviewer"),
    ];
    assert_eq!(labels(&all), ["reviewer", "beta:reviewer"]);
}

#[test]
fn a_plugin_subfolder_becomes_part_of_the_scoped_name() {
    let dir = plugin_dir(&[(
        "agents/review/security.md",
        "---\nname: security\ndescription: audits\n---\nAudit.",
    )]);
    let plugin = Plugin::at("my-plugin".to_string(), dir.path().to_owned());
    let [found] = of_plugin(&plugin, dir.path()).try_into().unwrap();
    assert_eq!(found.qualified(), "my-plugin:review:security");
}

#[test]
fn a_project_subfolder_does_not_change_the_name() {
    let dir = plugin_dir(&[(
        "review/security.md",
        "---\nname: sec\ndescription: audits\n---\nAudit.",
    )]);
    let [found] = of_dir(dir.path(), &Origin::Project).try_into().unwrap();
    assert_eq!(found.qualified(), "sec");
}

#[test]
fn disallowed_tools_remove_the_whole_tool_and_only_color_is_unapplied() {
    let dir = plugin_dir(&[(
        "agents/fixer.md",
        "---\nname: fixer\ntools: Read, Bash, Edit\ndisallowedTools: Bash(git push *)\nmodel: opus\ncolor: red\n---\nFix.",
    )]);
    let plugin = Plugin::at("demo".to_string(), dir.path().to_owned());
    let [found] = of_plugin(&plugin, dir.path()).try_into().unwrap();
    let scope = found.scope();
    assert!(scope.builtin(Builtin::Read));
    assert!(scope.builtin(Builtin::Edit));
    assert!(!scope.builtin(Builtin::RunCommand));
    assert!(!scope.builtin(Builtin::Write));
    assert!(!scope.mcp("github__create_issue"));
    assert_eq!(found.model, Some(Model::Alias(Alias::Opus)));
    assert_eq!(found.color.as_deref(), Some("red"));
    assert_eq!(found.unapplied(), ["color"]);
}

fn list(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_string()).collect()
}

#[test]
fn mcp_entries_follow_the_claude_code_naming() {
    let server = Scope::new(Some(list(&["Read", "mcp__github__*"]).as_slice()), None);
    assert!(server.mcp("github__create_issue"));
    assert!(!server.mcp("slack__post"));
    let exact = Scope::new(Some(list(&["mcp__reports__v2"]).as_slice()), None);
    assert!(!exact.mcp("reports__v2__run"));
    let withheld = Scope::new(None, Some(list(&["mcp__github"]).as_slice()));
    assert!(!withheld.mcp("github__create_issue"));
    assert!(withheld.mcp("slack__post"));
    assert!(withheld.builtin(Builtin::Edit));
    let no_mcp = Scope::new(None, Some(list(&["mcp__*"]).as_slice()));
    assert!(!no_mcp.mcp("slack__post"));
    let both = Scope::new(
        Some(list(&["Read"]).as_slice()),
        Some(list(&["Read"]).as_slice()),
    );
    assert!(!both.resolves());
}

#[test]
fn model_aliases_inherit_and_full_ids_parse() {
    assert_eq!(Model::parse("Sonnet"), Model::Alias(Alias::Sonnet));
    assert_eq!(Model::parse("haiku"), Model::Alias(Alias::Haiku));
    assert_eq!(Model::parse("inherit"), Model::Inherit);
    assert_eq!(
        Model::parse("claude-opus-5-5"),
        Model::Id("claude-opus-5-5".to_string())
    );
}

#[test]
fn project_files_without_a_name_or_description_are_skipped() {
    let dir = plugin_dir(&[
        ("notes.md", "# Notes about our agents\n"),
        ("bare.md", "---\nname: bare\n---\nNo description."),
        (
            "kept.md",
            "---\nname: kept\ndescription: kept\nskills: [lint, test, lint]\n---\nKept.",
        ),
    ]);
    let [found] = of_dir(dir.path(), &Origin::Project).try_into().unwrap();
    assert_eq!(found.name, "kept");
    assert_eq!(found.skills, ["lint", "test"]);
}

#[test]
fn the_closest_project_definition_wins() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".git")).unwrap();
    let inner = root.path().join("app");
    for (dir, body) in [(root.path(), "Outer."), (inner.as_path(), "Inner.")] {
        let agents = summonr_core::dirs::claude(dir).join(DIR);
        std::fs::create_dir_all(&agents).unwrap();
        std::fs::write(
            agents.join("reviewer.md"),
            format!("---\nname: reviewer\ndescription: reviews\n---\n{body}"),
        )
        .unwrap();
    }
    std::fs::write(
        summonr_core::dirs::claude(root.path())
            .join(DIR)
            .join("linter.md"),
        "---\nname: linter\ndescription: lints\n---\nLint.",
    )
    .unwrap();
    assert_eq!(find("linter", &inner).unwrap().instructions, "Lint.");
    assert_eq!(find("reviewer", &inner).unwrap().instructions, "Inner.");
    assert_eq!(
        find("reviewer", root.path()).unwrap().instructions,
        "Outer."
    );
}

#[test]
fn manifest_agents_replace_the_default_folder() {
    let dir = plugin_dir(&[
        (
            ".claude-plugin/plugin.json",
            r#"{"name": "demo", "agents": ["./custom/one.md"]}"#,
        ),
        ("custom/one.md", "---\nname: one\n---\nOne."),
        ("agents/two.md", "---\nname: two\n---\nTwo."),
    ]);
    let plugin = Plugin::at("demo".to_string(), dir.path().to_owned());
    let names: Vec<String> = of_plugin(&plugin, dir.path())
        .into_iter()
        .map(|agent| agent.name)
        .collect();
    assert_eq!(names, ["one"]);
}
