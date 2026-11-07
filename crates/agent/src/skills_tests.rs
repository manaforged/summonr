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
        super::load(workspace.path(), None, "probe-precedence")
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
        crate::plugins::commands::expand("/help", workspace.path(), None)
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
        super::load(workspace.path(), None, "probe-user-skill")
            .unwrap()
            .contains("user body")
    );
    assert!(
        super::load(workspace.path(), None, "probe-project-skill")
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
        crate::plugins::commands::expand(line, workspace.path(), Some("make it simple"))
            .expect("slash expansion")
            .expect("skill found")
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
    std::fs::write(dir.join("SKILL.md"), format!("---\ndescription: {about}\n---\n{about}\n"))
        .expect("skill body");
}

#[test]
fn a_skill_in_a_folder_lists_under_it_and_loads_by_its_own_name() {
    let workspace = tempfile::tempdir().expect("workspace");
    skill(workspace.path(), "probe-review/probe-rust", "Rust review");
    let catalog = super::catalog(workspace.path(), None).expect("skill catalog");
    assert!(catalog.contains("probe-rust [probe-review]: Rust review"), "{catalog}");
    assert!(
        super::load(workspace.path(), None, "probe-rust")
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
        super::load(workspace.path(), None, "probe-web:probe-deploy")
            .expect("skill body")
            .contains("Web deploy")
    );
}

#[test]
fn a_claude_code_skill_folder_is_read_one_level_deep() {
    let workspace = tempfile::tempdir().expect("workspace");
    let dir = workspace.path().join(".claude/skills/probe-group/probe-inner");
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
    let named = sets(
        "[lean]\ninclude = [\"probe-review\"]\nexclude = [\"probe-review/probe-panel\"]\n",
    );
    let lean = super::Set::from_sets(&named, "lean").expect("lean set");
    let catalog = super::catalog(workspace.path(), Some(&lean)).expect("skill catalog");
    assert!(catalog.contains("probe-rust"), "{catalog}");
    assert!(!catalog.contains("probe-panel"), "{catalog}");
    assert!(!catalog.contains("probe-other"), "{catalog}");
    let refused = super::load(workspace.path(), Some(&lean), "probe-panel")
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
