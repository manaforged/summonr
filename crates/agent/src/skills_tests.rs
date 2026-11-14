#[test]
fn skill_catalog_and_loading_use_the_same_root_precedence() {
    let workspace = tempfile::tempdir().expect("workspace");
    for (root, description) in [
        (".summonr", "Z preferred source"),
        (".claude", "A lower-priority source"),
    ] {
        let dir = workspace.path().join(root).join("skills/probe-precedence");
        std::fs::create_dir_all(&dir).expect("skill directory");
        std::fs::write(
            dir.join("SKILL.md"),
            format!("---\ndescription: {description}\n---\n{description}\n"),
        )
        .expect("skill body");
    }
    let catalog = super::catalog(workspace.path(), None).expect("skill catalog");
    assert!(
        catalog.contains("probe-precedence: Z preferred source"),
        "{catalog}"
    );
    assert!(!catalog.contains("A lower-priority source"), "{catalog}");
    assert!(
        load(workspace.path(), None, "probe-precedence", None)
            .expect("skill body")
            .contains("Z preferred source")
    );
}

#[test]
fn a_skill_cannot_shadow_a_builtin_slash_command() {
    let workspace = tempfile::tempdir().expect("workspace");
    let dir = workspace.path().join(".claude/skills/help");
    std::fs::create_dir_all(&dir).expect("skill directory");
    std::fs::write(dir.join("SKILL.md"), "skill instructions").expect("skill body");
    assert!(
        crate::plugins::commands::offers(workspace.path())
            .iter()
            .all(|offer| offer.insert != "/help")
    );
    assert!(
        crate::plugins::commands::expand("/help", workspace.path(), None, None)
            .expect("slash expansion")
            .is_none()
    );
}

#[test]
fn claude_code_skills_join_the_catalog_and_load() {
    let home = summonr_core::dirs::user_home().expect("test home");
    let user = home.join(".claude/skills/probe-user-skill");
    std::fs::create_dir_all(&user).unwrap();
    std::fs::write(
        user.join("SKILL.md"),
        "---\nname: probe-user-skill\ndescription: from the user's Claude folder\n---\nuser body\n",
    )
    .unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let project = workspace.path().join(".claude/skills/probe-project-skill");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("SKILL.md"),
        "---\nname: probe-project-skill\ndescription: from the project's Claude folder\n---\nproject body\n",
    )
    .unwrap();
    let catalog = super::catalog(workspace.path(), None).unwrap_or_default();
    assert!(
        catalog.contains("probe-user-skill: from the user's Claude folder"),
        "{catalog}"
    );
    assert!(
        catalog.contains("probe-project-skill: from the project's Claude folder"),
        "{catalog}"
    );
    assert!(
        load(workspace.path(), None, "probe-user-skill", None)
            .unwrap()
            .contains("user body")
    );
    assert!(
        load(workspace.path(), None, "probe-project-skill", None)
            .unwrap()
            .contains("project body")
    );
}

#[test]
fn a_skill_with_a_binary_body_still_lists() {
    let workspace = tempfile::tempdir().unwrap();
    let skill = workspace.path().join(".claude/skills/probe-binary-skill");
    std::fs::create_dir_all(&skill).unwrap();
    let mut body =
        b"---\nname: probe-binary-skill\ndescription: listed from its frontmatter\n---\n".to_vec();
    body.extend([0xff, 0xfe, 0x00, 0xc3, 0x28]);
    std::fs::write(skill.join("SKILL.md"), body).unwrap();
    let catalog = super::catalog(workspace.path(), None).unwrap_or_default();
    assert!(
        catalog.contains("probe-binary-skill: listed from its frontmatter"),
        "{catalog}"
    );
}

#[test]
fn a_skill_with_no_arguments_runs_on_the_previous_task() {
    let workspace = tempfile::tempdir().expect("workspace");
    let dir = workspace.path().join(".claude/skills/probe-task");
    std::fs::create_dir_all(&dir).expect("skill directory");
    std::fs::write(dir.join("SKILL.md"), "Run on: $ARGUMENTS").expect("skill body");
    let expand = |line: &str| {
        crate::plugins::commands::expand(line, workspace.path(), Some("make it simple"), None)
            .expect("slash expansion")
            .expect("skill found")
            .written()
    };
    assert_eq!(expand("/probe-task"), "Run on: make it simple");
    assert_eq!(
        expand("/probe-task review the api"),
        "Run on: review the api"
    );
}

fn skill(root: &std::path::Path, path: &str, about: &str) {
    let dir = root.join(".summonr").join("skills").join(path);
    std::fs::create_dir_all(&dir).expect("skill directory");
    std::fs::write(
        dir.join("SKILL.md"),
        format!("---\ndescription: {about}\n---\n{about}\n"),
    )
    .expect("skill body");
}

#[test]
fn a_skill_in_a_folder_lists_under_it_and_loads_by_its_own_name() {
    let workspace = tempfile::tempdir().expect("workspace");
    skill(workspace.path(), "probe-review/probe-rust", "Rust review");
    let catalog = super::catalog(workspace.path(), None).expect("skill catalog");
    assert!(
        catalog.contains("probe-rust [probe-review]: Rust review"),
        "{catalog}"
    );
    assert!(
        load(workspace.path(), None, "probe-rust", None)
            .expect("skill body")
            .contains("Rust review")
    );
}

#[test]
fn two_skills_with_one_name_in_two_folders_both_list_by_folder() {
    let workspace = tempfile::tempdir().expect("workspace");
    skill(workspace.path(), "probe-ops/probe-deploy", "Ops deploy");
    skill(workspace.path(), "probe-web/probe-deploy", "Web deploy");
    let names: Vec<String> = super::list(workspace.path())
        .into_iter()
        .map(|(name, _)| name)
        .filter(|name| name.contains("probe-deploy"))
        .collect();
    assert_eq!(names, ["probe-ops:probe-deploy", "probe-web:probe-deploy"]);
    assert!(
        load(workspace.path(), None, "probe-web:probe-deploy", None)
            .expect("skill body")
            .contains("Web deploy")
    );
}

#[test]
fn a_claude_code_skill_folder_is_read_one_level_deep() {
    let workspace = tempfile::tempdir().expect("workspace");
    let dir = workspace
        .path()
        .join(".claude/skills/probe-group/probe-inner");
    std::fs::create_dir_all(&dir).expect("skill directory");
    std::fs::write(dir.join("SKILL.md"), "inner").expect("skill body");
    assert!(
        super::list(workspace.path())
            .iter()
            .all(|(name, _)| !name.contains("probe-inner"))
    );
}

fn sets(text: &str) -> std::collections::BTreeMap<String, super::Set> {
    toml::from_str(text).expect("skill sets")
}

#[test]
fn a_skill_set_shows_its_folders_and_refuses_a_left_out_skill() {
    let workspace = tempfile::tempdir().expect("workspace");
    skill(workspace.path(), "probe-review/probe-rust", "Rust review");
    skill(workspace.path(), "probe-review/probe-panel", "Panel review");
    skill(workspace.path(), "probe-other", "Other");
    let named =
        sets("[lean]\ninclude = [\"probe-review\"]\nexclude = [\"probe-review/probe-panel\"]\n");
    let lean = super::Set::from_sets(&named, "lean").expect("lean set");
    let catalog = super::catalog(workspace.path(), Some(&lean)).expect("skill catalog");
    assert!(catalog.contains("probe-rust"), "{catalog}");
    assert!(!catalog.contains("probe-panel"), "{catalog}");
    assert!(!catalog.contains("probe-other"), "{catalog}");
    let refused = load(workspace.path(), Some(&lean), "probe-panel", None)
        .expect_err("left out of the set")
        .to_string();
    assert!(refused.contains("not in skill set `lean`"), "{refused}");
}

#[test]
fn the_none_set_hides_every_skill_and_an_unknown_set_names_the_table() {
    let workspace = tempfile::tempdir().expect("workspace");
    skill(workspace.path(), "probe-other", "Other");
    let none = super::Set::from_sets(&sets(""), "none").expect("built-in none set");
    assert_eq!(super::catalog(workspace.path(), Some(&none)), None);
    let missing = super::Set::from_sets(&sets(""), "lean")
        .expect_err("no such set")
        .to_string();
    assert!(missing.contains("[skills.sets.lean]"), "{missing}");
}

#[test]
fn a_folder_path_picks_its_skills_with_no_set_in_config() {
    let workspace = tempfile::tempdir().expect("workspace");
    skill(workspace.path(), "probe-acme/probe-deploy", "Deploy");
    skill(workspace.path(), "probe-mine/probe-notes", "Notes");
    let acme = super::Set::resolve(&sets(""), "probe-acme", workspace.path()).expect("folder");
    let catalog = super::catalog(workspace.path(), Some(&acme)).expect("acme catalog");
    assert!(catalog.contains("probe-deploy"), "{catalog}");
    assert!(!catalog.contains("probe-notes"), "{catalog}");
    let both = super::Set::resolve(&sets(""), "probe-acme,probe-mine", workspace.path())
        .expect("two folders");
    let catalog = super::catalog(workspace.path(), Some(&both)).expect("both catalog");
    assert!(catalog.contains("probe-notes"), "{catalog}");
    let missing = super::Set::resolve(&sets(""), "probe-nowhere", workspace.path())
        .expect_err("no such folder")
        .to_string();
    assert!(missing.contains("skill folder"), "{missing}");
}

fn write(path: &std::path::Path, text: &str) {
    std::fs::create_dir_all(path.parent().expect("parent")).expect("skill directory");
    std::fs::write(path, text).expect("skill body");
}

fn load(
    workspace: &std::path::Path,
    set: Option<&super::Set>,
    name: &str,
    session: Option<&str>,
) -> summonr_core::error::Result<String> {
    super::load(workspace, set, name, session).map(|body| body.written())
}

fn typed(workspace: &std::path::Path, line: &str) -> Option<String> {
    crate::plugins::commands::expand(line, workspace, None, None)
        .expect("slash expansion")
        .map(|body| body.written())
}

#[test]
fn a_plugin_skill_answers_to_its_plugin_name_beside_a_same_named_user_skill() {
    let plugins = summonr_core::dirs::summonr_home().unwrap().join("plugins");
    let plugin = plugins.join("probe-plug");
    write(
        &plugin.join("skills/probe-same/SKILL.md"),
        "---\ndescription: plugin same\n---\nroot ${CLAUDE_PLUGIN_ROOT} dir ${CLAUDE_SKILL_DIR} data ${CLAUDE_PLUGIN_DATA} $ARGUMENTS",
    );
    write(
        &plugin.join("skills/probe-dir/SKILL.md"),
        "---\nname: probe-plug:probe-fancy\n---\nfancy body",
    );
    write(
        &plugins.join("probe-solo/SKILL.md"),
        "---\ndescription: solo\n---\nsolo body",
    );
    let workspace = tempfile::tempdir().unwrap();
    write(
        &workspace.path().join(".claude/skills/probe-same/SKILL.md"),
        "---\ndescription: local same\n---\nlocal body",
    );
    let names: Vec<String> = super::list(workspace.path())
        .into_iter()
        .map(|(name, _)| name)
        .filter(|name| name.contains("probe-"))
        .collect();
    for name in [
        "probe-same",
        "probe-plug:probe-same",
        "probe-plug:probe-fancy",
        "probe-fancy",
        "probe-solo:probe-solo",
        "probe-solo",
    ] {
        assert_eq!(
            names.iter().filter(|seen| *seen == name).count(),
            1,
            "{name}: {names:?}"
        );
    }
    let data = summonr_core::dirs::summonr_home()
        .unwrap()
        .join("plugin-data/probe-plug");
    assert_eq!(
        typed(workspace.path(), "/probe-plug:probe-same go").as_deref(),
        Some(
            format!(
                "root {} dir {} data {} go",
                plugin.display(),
                plugin.join("skills/probe-same").display(),
                data.display()
            )
            .as_str()
        )
    );
    assert_eq!(
        typed(workspace.path(), "/probe-same").as_deref(),
        Some("local body")
    );
    assert!(
        load(workspace.path(), None, "probe-fancy", None)
            .unwrap()
            .contains("fancy body")
    );
    assert_eq!(
        typed(workspace.path(), "/probe-solo").as_deref(),
        Some("solo body")
    );
}

#[test]
fn invocation_flags_split_the_model_catalog_from_the_slash_menu() {
    let workspace = tempfile::tempdir().unwrap();
    let skills = workspace.path().join(".claude/skills");
    write(
        &skills.join("probe-manual/SKILL.md"),
        "---\ndescription: manual\ndisable-model-invocation: true\n---\nmanual body",
    );
    write(
        &skills.join("probe-quiet/SKILL.md"),
        "---\ndescription: quiet\nuser-invocable: off\n---\nquiet body",
    );
    let catalog = super::catalog(workspace.path(), None).unwrap_or_default();
    assert!(!catalog.contains("probe-manual"), "{catalog}");
    assert!(catalog.contains("probe-quiet: quiet"), "{catalog}");
    let refused = load(workspace.path(), None, "probe-manual", None)
        .expect_err("model invocation is off")
        .to_string();
    assert!(refused.contains("/probe-manual"), "{refused}");
    assert_eq!(
        typed(workspace.path(), "/probe-manual").as_deref(),
        Some("manual body")
    );
    assert!(
        super::list(workspace.path())
            .iter()
            .all(|(name, _)| name != "probe-quiet")
    );
    assert_eq!(typed(workspace.path(), "/probe-quiet now"), None);
    assert!(
        load(workspace.path(), None, "probe-quiet", None)
            .unwrap()
            .contains("quiet body")
    );
}

#[test]
fn frontmatter_names_the_skill_and_declares_its_arguments() {
    let workspace = tempfile::tempdir().unwrap();
    let file = workspace
        .path()
        .join(".claude/skills/probe-folder-name/SKILL.md");
    write(
        &file,
        "---\nname: probe-named\ndescription: Short.\nwhen_to_use: When asked.\narguments: [issue, branch]\nallowed-tools: Bash(git add *) Read\nmodel: probe-model\n---\nFix $issue on $branch",
    );
    write(
        &workspace.path().join(".claude/skills/probe-plain/SKILL.md"),
        "Plain body",
    );
    let catalog = super::catalog(workspace.path(), None).unwrap_or_default();
    assert!(
        catalog.contains("probe-named: Short. When asked."),
        "{catalog}"
    );
    assert_eq!(
        typed(workspace.path(), "/probe-named 12").as_deref(),
        Some("Fix 12 on ")
    );
    assert_eq!(
        typed(workspace.path(), "/probe-folder-name 12 main").as_deref(),
        Some("Fix 12 on main")
    );
    assert_eq!(
        typed(workspace.path(), "/probe-plain extra words").as_deref(),
        Some("Plain body\n\nARGUMENTS: extra words")
    );
    let meta = super::Meta::read(&file);
    assert_eq!(meta.allowed_tools, ["Bash(git add *)", "Read"]);
    assert_eq!(meta.model.as_deref(), Some("probe-model"));
}
