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
    let catalog = super::catalog(workspace.path()).expect("skill catalog");
    assert!(
        catalog.contains("probe-precedence: Z preferred source"),
        "{catalog}"
    );
    assert!(!catalog.contains("A lower-priority source"), "{catalog}");
    assert!(
        super::load(workspace.path(), "probe-precedence")
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
        crate::plugins::commands::expand("/help", workspace.path())
            .expect("slash expansion")
            .is_none()
    );
}

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

#[test]
fn a_skill_with_a_binary_body_still_lists() {
    let workspace = tempfile::tempdir().unwrap();
    let skill = workspace.path().join(".claude/skills/probe-binary-skill");
    std::fs::create_dir_all(&skill).unwrap();
    let mut body =
        b"---\nname: probe-binary-skill\ndescription: listed from its frontmatter\n---\n".to_vec();
    body.extend([0xff, 0xfe, 0x00, 0xc3, 0x28]);
    std::fs::write(skill.join("SKILL.md"), body).unwrap();
    let catalog = super::catalog(workspace.path()).unwrap_or_default();
    assert!(
        catalog.contains("probe-binary-skill: listed from its frontmatter"),
        "{catalog}"
    );
}
