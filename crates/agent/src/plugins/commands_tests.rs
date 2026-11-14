use super::*;

fn command(dir: &Path, template: &str) -> Command {
    let plugin = Plugin::at("demo".to_string(), dir.to_owned());
    Command::new(&plugin, "echo".to_string(), &Frontmatter::parse(template)).unwrap()
}

fn written(body: Result<Option<Body>>) -> Option<String> {
    body.unwrap().map(|body| body.written())
}

fn fill(template: &str, arguments: &str, names: &[String]) -> String {
    Body::render(
        template,
        Some((arguments, names)),
        Scope::User,
        str::to_owned,
    )
    .written()
}

#[test]
fn argument_text_is_never_rescanned_but_path_variables_still_fill() {
    let dir = tempfile::tempdir().unwrap();
    let rendered = command(dir.path(), "All: $ARGUMENTS. First: $0. Third: $2.").render(
        "price $1 ${CLAUDE_PLUGIN_ROOT}",
        dir.path(),
        None,
    );
    let rendered = rendered.written();
    let root = dir.path().display();
    assert_eq!(
        rendered,
        format!("All: price $1 {root}. First: price. Third: {root}.")
    );
}

#[test]
fn the_plugin_root_expands_in_the_template_only() {
    let dir = tempfile::tempdir().unwrap();
    let rendered =
        command(dir.path(), "Run ${CLAUDE_PLUGIN_ROOT}/bin/lint $0").render("x", dir.path(), None);
    assert_eq!(
        rendered.written(),
        format!("Run {}/bin/lint x", dir.path().display())
    );
}

#[test]
fn the_session_id_fills_a_command() {
    let dir = tempfile::tempdir().unwrap();
    let rendered =
        command(dir.path(), "Log to ${CLAUDE_SESSION_ID}.log").render("", dir.path(), Some("s-9"));
    assert_eq!(rendered.written(), "Log to s-9.log");
}

#[test]
fn a_quoted_argument_binds_as_one_word() {
    let dir = tempfile::tempdir().unwrap();
    let rendered =
        command(dir.path(), "say $0 then $1").render("\"hello world\" next", dir.path(), None);
    assert_eq!(rendered.written(), "say hello world then next");
}

#[test]
fn a_project_command_file_expands_with_its_arguments() {
    let workspace = tempfile::tempdir().unwrap();
    let folder = workspace.path().join(".summonr/commands");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join("ship-check.md"),
        "---\ndescription: check a crate\n---\nRun the tests for $0, then report: $ARGUMENTS",
    )
    .unwrap();
    let expanded = written(expand(
        "/ship-check summonr fast",
        workspace.path(),
        None,
        None,
    ));
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
        "Deploy with deploy-key-value-42 to $0",
    )
    .unwrap();
    std::fs::write(
        root.join(".summonr/skills/release.md"),
        "Sign with deploy-key-value-42",
    )
    .unwrap();
    assert_eq!(
        written(expand("/deploy staging", root, None, None)).as_deref(),
        Some("Deploy with [secret] to staging")
    );
    assert_eq!(
        written(expand("/release", root, None, None)).as_deref(),
        Some("Sign with [secret]")
    );
}

#[test]
fn placeholders_follow_the_claude_code_substitution_rules() {
    let names = vec!["issue".to_string(), "branch".to_string()];
    let filled = fill(
        "$ARGUMENTS[1] then $0, missing $5, \\$1.00, \\\\$0, $issue on $branch$ARGUMENTSx",
        "\"a b\" c",
        &names,
    );
    assert_eq!(
        filled,
        "c then a b, missing $5, $1.00, \\\\a b, a b on c\"a b\" cx"
    );
    let unnamed = fill("$issue_id and $branch", "only", &names);
    assert_eq!(unnamed, "$issue_id and ");
}

#[test]
fn arguments_no_placeholder_takes_are_appended() {
    assert_eq!(
        fill("Review it. Cost $9.", "the parser", &[]),
        "Review it. Cost $9.\n\nARGUMENTS: the parser"
    );
    assert_eq!(fill("Review it.", "", &[]), "Review it.");
    assert_eq!(fill("Review $0.", "the parser", &[]), "Review the.");
}

#[test]
fn a_command_in_a_subfolder_is_named_by_its_folder_path() {
    let workspace = tempfile::tempdir().unwrap();
    let folder = workspace
        .path()
        .join(".claude/commands/probe-front/probe-deep");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join("probe-widget.md"),
        "---\nargument-hint: [name]\n---\nBuild $0",
    )
    .unwrap();
    let expanded = written(expand(
        "/probe-front:probe-deep:probe-widget Card",
        workspace.path(),
        None,
        None,
    ));
    assert_eq!(expanded.as_deref(), Some("Build Card"));
    let offered = offers(workspace.path());
    let offer = offered
        .iter()
        .find(|offer| offer.insert == "/probe-front:probe-deep:probe-widget")
        .expect("subfolder command offered");
    assert_eq!(offer.about, "Build $0 [name]");
}

#[test]
fn the_manifest_commands_key_replaces_the_default_folder() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join(".claude-plugin")).unwrap();
    std::fs::create_dir_all(root.join("commands")).unwrap();
    std::fs::create_dir_all(root.join("extra/nested")).unwrap();
    std::fs::write(root.join("commands/skipped.md"), "not loaded").unwrap();
    std::fs::write(root.join("extra/nested/deep.md"), "deep body").unwrap();
    std::fs::write(
        root.join("status.md"),
        "---\ndescription: from file\n---\nStatus of $0",
    )
    .unwrap();
    let manifests = [
        (r#"{"commands": "./extra"}"#, vec!["nested:deep"]),
        (
            r#"{"commands": ["./status.md", "./extra/"]}"#,
            vec!["nested:deep", "status"],
        ),
        (
            r#"{"commands": {"status": {"source": "./status.md", "argumentHint": "[env]", "model": "probe-model", "allowedTools": ["Read"]}, "about": {"content": "Explain it."}, "both": {"source": "./status.md", "content": "x"}}}"#,
            vec!["about", "status"],
        ),
    ];
    for (manifest, names) in manifests {
        std::fs::write(root.join(".claude-plugin/plugin.json"), manifest).unwrap();
        let loaded = load(&Plugin::at("probe-kit".to_string(), root.to_owned()));
        let found: Vec<&str> = loaded.iter().map(|command| command.name.as_str()).collect();
        assert_eq!(found, names, "{manifest}");
    }
    let loaded = load(&Plugin::at("probe-kit".to_string(), root.to_owned()));
    let status = loaded
        .iter()
        .find(|command| command.name == "status")
        .unwrap();
    assert_eq!(status.meta.offer(), "from file [env]");
    assert_eq!(status.meta.model.as_deref(), Some("probe-model"));
    assert_eq!(status.meta.allowed_tools, ["Read"]);
    assert_eq!(status.qualified(), "/probe-kit:status");
    assert_eq!(
        status.render("prod", root, None).written(),
        "Status of prod"
    );
}

#[test]
fn a_command_that_is_not_user_invocable_does_not_run() {
    let workspace = tempfile::tempdir().unwrap();
    let folder = workspace.path().join(".claude/commands");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join("probe-hidden.md"),
        "---\nuser-invocable: No\n---\nhidden",
    )
    .unwrap();
    assert_eq!(
        written(expand("/probe-hidden", workspace.path(), None, None)),
        None
    );
}

#[test]
fn a_typed_command_gets_its_block_output_and_keeps_arguments_out_of_the_shell() {
    let workspace = tempfile::tempdir().unwrap();
    let folder = workspace.path().join(".summonr/commands");
    std::fs::create_dir_all(&folder).unwrap();
    let template = "Status: !`echo typed-out $ARGUMENTS` for $0";
    std::fs::write(folder.join("probe-shell.md"), template).unwrap();
    let body = expand("/probe-shell main", workspace.path(), None, None)
        .unwrap()
        .unwrap();
    let policy = crate::policy::Policy::builtin();
    let mut tools =
        crate::tools::ToolRegistry::with_policy_and_search(workspace.path(), policy, None).unwrap();
    tools.set_trust(true);
    let input = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap()
        .block_on(tools.inject(&body))
        .unwrap();
    assert_eq!(input.text, "Status: typed-out for main");
}

#[test]
fn the_session_id_fills_a_skill_the_model_loads_or_a_person_types() {
    let workspace = tempfile::tempdir().expect("workspace");
    let dir = workspace.path().join(".claude/skills/probe-log");
    std::fs::create_dir_all(&dir).expect("skill directory");
    std::fs::write(dir.join("SKILL.md"), "Log to logs/${CLAUDE_SESSION_ID}.log").expect("skill");
    let load = |session| {
        skills::load(workspace.path(), None, "probe-log", session)
            .expect("load")
            .written()
    };
    assert_eq!(load(Some("s-7")), "Log to logs/s-7.log");
    let typed = written(expand("/probe-log", workspace.path(), None, Some("s-7")));
    assert_eq!(typed.as_deref(), Some("Log to logs/s-7.log"));
    assert_eq!(load(None), "Log to logs/${CLAUDE_SESSION_ID}.log");
}
