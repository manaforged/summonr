use super::*;

fn agent(origin: Origin, name: &str) -> Agent {
    Agent {
        origin,
        name: name.to_string(),
        description: String::new(),
        tools: None,
        disallowed: None,
        model: None,
        color: None,
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
fn disallowed_tools_leave_the_list_and_unapplied_fields_are_named() {
    let dir = plugin_dir(&[(
        "agents/fixer.md",
        "---\nname: fixer\ntools: Read, Bash, Edit\ndisallowedTools: Bash(git push *)\nmodel: opus\ncolor: red\n---\nFix.",
    )]);
    let plugin = Plugin::at("demo".to_string(), dir.path().to_owned());
    let [found] = of_plugin(&plugin, dir.path()).try_into().unwrap();
    assert_eq!(
        found.tools,
        Some(vec!["Read".to_string(), "Edit".to_string()])
    );
    assert_eq!(found.model.as_deref(), Some("opus"));
    assert_eq!(found.color.as_deref(), Some("red"));
    assert_eq!(found.unapplied(), ["model", "color"]);
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
