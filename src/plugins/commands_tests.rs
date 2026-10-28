use super::*;

fn command(dir: &Path, template: &str) -> Command {
    Command {
        plugin: Plugin::at("demo".to_string(), dir.to_owned()),
        name: "echo".to_string(),
        description: String::new(),
        hint: String::new(),
        template: template.to_string(),
    }
}

#[test]
fn arguments_are_substituted_once_and_never_rescanned() {
    let dir = tempfile::tempdir().unwrap();
    let rendered = command(dir.path(), "All: $ARGUMENTS. First: $1. Third: $3.")
        .render("price $1 ${CLAUDE_PLUGIN_ROOT}", dir.path());
    assert_eq!(
        rendered,
        "All: price $1 ${CLAUDE_PLUGIN_ROOT}. First: price. Third: ${CLAUDE_PLUGIN_ROOT}."
    );
}

#[test]
fn the_plugin_root_expands_in_the_template_only() {
    let dir = tempfile::tempdir().unwrap();
    let rendered =
        command(dir.path(), "Run ${CLAUDE_PLUGIN_ROOT}/bin/lint $1").render("x", dir.path());
    assert_eq!(rendered, format!("Run {}/bin/lint x", dir.path().display()));
}

#[test]
fn a_quoted_argument_binds_as_one_word() {
    let dir = tempfile::tempdir().unwrap();
    let rendered = command(dir.path(), "say $1 then $2").render("\"hello world\" next", dir.path());
    assert_eq!(rendered, "say hello world then next");
}

#[test]
fn a_project_command_file_expands_with_its_arguments() {
    let workspace = tempfile::tempdir().unwrap();
    let folder = workspace.path().join(".summonr/commands");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join("ship-check.md"),
        "---\ndescription: check a crate\n---\nRun the tests for $1, then report: $ARGUMENTS",
    )
    .unwrap();
    let expanded = expand("/ship-check summonr fast", workspace.path()).unwrap();
    assert_eq!(
        expanded.as_deref(),
        Some("Run the tests for summonr, then report: summonr fast")
    );
}

#[test]
fn a_workspace_secret_never_reaches_the_prompt_through_a_command_or_a_skill() {
    let workspace = tempfile::tempdir().unwrap();
    let root = workspace.path();
    std::fs::write(root.join(".env"), "API_KEY=deploy-key-value-42\n").unwrap();
    std::fs::create_dir_all(root.join(".summonr/commands")).unwrap();
    std::fs::create_dir_all(root.join(".summonr/skills")).unwrap();
    std::fs::write(
        root.join(".summonr/commands/deploy.md"),
        "Deploy with deploy-key-value-42 to $1",
    )
    .unwrap();
    std::fs::write(
        root.join(".summonr/skills/release.md"),
        "Sign with deploy-key-value-42",
    )
    .unwrap();
    assert_eq!(
        expand("/deploy staging", root).unwrap().as_deref(),
        Some("Deploy with [secret] to staging")
    );
    assert_eq!(
        expand("/release", root).unwrap().as_deref(),
        Some("Sign with [secret]")
    );
}
