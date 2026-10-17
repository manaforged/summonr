#[test]
fn claude_code_skills_join_the_catalog_and_load() {
    let home = crate::prompt::user_home().expect("test home");
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
    let catalog = super::catalog(workspace.path()).unwrap_or_default();
    assert!(
        catalog.contains("probe-user-skill: from the user's Claude folder"),
        "{catalog}"
    );
    assert!(
        catalog.contains("probe-project-skill: from the project's Claude folder"),
        "{catalog}"
    );
    assert!(
        super::load(workspace.path(), "probe-user-skill")
            .unwrap()
            .contains("user body")
    );
    assert!(
        super::load(workspace.path(), "probe-project-skill")
            .unwrap()
            .contains("project body")
    );
}
