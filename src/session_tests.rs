use super::*;
use crate::durable::id::SessionId;
use crate::durable::log::{self, Line, Log};
use crate::error::Result;

#[test]
fn prompt_search_reads_original_text_across_sessions_and_skips_worker_inputs() -> Result<()> {
    let home = tempfile::tempdir()?;
    let mut ids = Vec::new();
    for (depth, name) in [(0, "Parser repair"), (0, "Release notes"), (1, "Worker")] {
        let id = SessionId::new();
        let path = log::session_path(home.path(), id);
        let mut log = Log::create(&path)?;
        log.append(&Line::Session {
            id,
            model: "test-model".to_string(),
            system: "test".to_string(),
            workspace: home.path().join(name),
            parent: None,
            depth,
        })?;
        catalog::set_name(&path, name)?;
        log.append(&Line::User {
            text: "expanded file contents\n\nHarness state: internal".to_string(),
            source: Some(format!("Review 🦀 {name}")),
            images: vec![crate::provider::Image::encode("image/png", b"fixture")],
            start: true,
        })?;
        log.append(&Line::User {
            text: "internal continuation".to_string(),
            source: None,
            images: Vec::new(),
            start: false,
        })?;
        log.append(&Line::Close { reason: None })?;
        log.append(&Line::Clear)?;
        log.append(&Line::User {
            text: "second request\n\nHarness state: hidden".to_string(),
            source: None,
            images: Vec::new(),
            start: true,
        })?;
        ids.push(id.to_string());
    }
    let (prompts, skipped) = catalog::prompts_in(home.path(), None)?;
    assert!(skipped.is_empty());
    assert_eq!(prompts.len(), 4);
    assert_eq!(prompts[0].session.id, ids[1]);
    assert_eq!(prompts[0].turn, 1);
    assert_eq!(prompts[0].text, "second request");
    assert!(!prompts[0].closed);
    assert!(prompts[1].closed);
    assert_eq!(prompts[1].text, "Review 🦀 Release notes");
    assert_eq!(prompts[1].images, 1);
    assert_eq!(prompts[2].session.id, ids[0]);
    assert_eq!(prompts[3].text, "Review 🦀 Parser repair");
    assert!(
        !prompts
            .iter()
            .any(|prompt| prompt.text.contains("Harness state"))
    );
    let broken = log::session_path(home.path(), SessionId::new());
    std::fs::create_dir_all(broken.parent().expect("session directory"))?;
    std::fs::write(&broken, "{\"kind\":\"damaged_record\"}\n")?;
    let (healthy, skipped) = catalog::prompts_in(home.path(), None)?;
    assert_eq!(
        healthy, prompts,
        "a damaged journal does not hide healthy prompts"
    );
    assert_eq!(skipped.len(), 1);
    assert!(skipped[0].contains(&broken.display().to_string()));
    assert!(skipped[0].contains("damaged_record"));
    let (branch, skipped) = catalog::prompts_in(home.path(), Some(&ids[0]))?;
    assert!(skipped.is_empty());
    assert_eq!(branch, prompts[2..]);
    let broken_id = broken
        .parent()
        .expect("session directory")
        .file_name()
        .expect("session id")
        .to_string_lossy();
    assert!(
        catalog::prompts_in(home.path(), Some(&broken_id))
            .expect_err("selected damaged journal fails")
            .to_string()
            .contains("damaged_record")
    );
    assert!(
        catalog::prompts_in(home.path(), Some("invalid"))
            .expect_err("invalid ID fails")
            .to_string()
            .contains("Invalid session ID")
    );
    Ok(())
}

#[test]
fn last_session_matches_its_workspace() -> Result<()> {
    let home = tempfile::tempdir()?;
    let workspace = home.path().join("workspace");
    std::fs::create_dir(&workspace)?;
    let driver =
        crate::durable::driver::SessionDriver::open(home.path(), &workspace, "test-model", "test")
            .map_err(|error| crate::Error::Config(error.to_string()))?;
    let path = driver.path().to_path_buf();
    drop(driver);

    assert_eq!(
        catalog::last_in(home.path(), &workspace)?,
        Some(path.clone())
    );
    let other = home.path().join("other");
    std::fs::create_dir(&other)?;
    assert_eq!(catalog::last_in(home.path(), &other)?, None);
    let listed = catalog::list_in(home.path())?;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].path, path);
    assert_eq!(listed[0].model.as_deref(), Some("test-model"));
    assert_eq!(
        listed[0].cwd.as_deref().map(std::path::Path::new),
        Some(workspace.canonicalize()?.as_path())
    );
    Ok(())
}

#[test]
fn child_session_does_not_replace_the_default_resume_target() -> Result<()> {
    let home = tempfile::tempdir()?;
    let workspace = home.path().join("workspace");
    std::fs::create_dir(&workspace)?;
    let parent = crate::durable::driver::SessionDriver::open(
        home.path(),
        &workspace,
        "parent-model",
        "test",
    )
    .map_err(|error| crate::Error::Config(error.to_string()))?;
    let parent_path = parent.path().to_path_buf();
    let child = crate::durable::driver::SessionDriver::open_child(
        home.path(),
        &workspace,
        "child-model",
        "test",
        parent.id(),
    )
    .map_err(|error| crate::Error::Config(error.to_string()))?;

    assert_ne!(child.path(), parent_path);
    assert_eq!(
        catalog::last_in(home.path(), &workspace)?,
        Some(parent_path)
    );
    Ok(())
}

#[test]
fn previous_session_skips_the_current_session() -> Result<()> {
    let home = tempfile::tempdir()?;
    let workspace = home.path().join("workspace");
    std::fs::create_dir(&workspace)?;
    let first =
        crate::durable::driver::SessionDriver::open(home.path(), &workspace, "first-model", "test")
            .map_err(|error| crate::Error::Config(error.to_string()))?;
    let first_id = first.id().to_string();
    drop(first);
    let current = crate::durable::driver::SessionDriver::open(
        home.path(),
        &workspace,
        "current-model",
        "test",
    )
    .map_err(|error| crate::Error::Config(error.to_string()))?;
    let current_id = current.id().to_string();
    drop(current);

    let previous = catalog::previous_in(home.path(), &workspace, &current_id)?
        .unwrap_or_else(|| panic!("previous session"));

    assert_eq!(previous.id, first_id);
    assert_eq!(previous.model.as_deref(), Some("first-model"));
    Ok(())
}

#[test]
fn depth_zero_fork_remains_a_default_resume_target() -> Result<()> {
    let home = tempfile::tempdir()?;
    let workspace = home.path().join("workspace");
    std::fs::create_dir(&workspace)?;
    let parent = crate::durable::driver::SessionDriver::open(
        home.path(),
        &workspace,
        "parent-model",
        "test",
    )
    .map_err(|error| crate::Error::Config(error.to_string()))?;
    let fork = parent
        .fork(None)
        .map_err(|error| crate::Error::Config(error.to_string()))?;

    assert_eq!(catalog::last_in(home.path(), &workspace)?, Some(fork));
    assert_eq!(
        catalog::list_in(home.path())?
            .last()
            .map(|session| session.depth),
        Some(0)
    );
    Ok(())
}

#[test]
fn harness_text_follows_the_user_text() {
    assert_eq!(decorate("control", "request"), "request\n\ncontrol");
    assert_eq!(decorate("control", ""), "control");
}

#[test]
fn session_prefixes_skip_the_leading_zeros_and_previews_drop_the_harness_note() {
    use super::catalog::{prefix, preview, short};
    let id = "000000000000000018d04a270eedcd68";
    assert!(prefix(id, "18d0"));
    assert!(prefix(id, "0018d0"));
    assert!(prefix(id, id));
    assert!(!prefix(id, "19"));
    assert_eq!(short(id), "18d04a270eedcd68");
    assert_eq!(short("0000"), "0000");
    assert_eq!(
        preview("fix the tests\n\nHarness state: implementation mode is active."),
        "fix the tests"
    );
}

#[test]
fn a_named_session_lists_resolves_and_exports() -> Result<()> {
    use crate::durable::driver::{Exposure, SessionDriver};
    use crate::durable::log::Usage;
    let home = tempfile::tempdir()?;
    let workspace = home.path().join("workspace");
    std::fs::create_dir(&workspace)?;
    let mut driver = SessionDriver::open(home.path(), &workspace, "test-model", "test")
        .map_err(|error| crate::Error::Config(error.to_string()))?;
    let path = driver.path().to_path_buf();
    driver
        .prepare_turn_work("fix the widget", &[])
        .map_err(|error| crate::Error::Config(error.to_string()))?;
    drop(
        driver
            .start_input(&"fix the widget\n\nHarness state: implementation mode is active.".into())
            .map_err(|error| crate::Error::Config(error.to_string()))?,
    );
    let attempt = driver
        .start_provider(
            "test-model",
            Exposure {
                max_input_tokens: 1000,
                max_output_tokens: 100,
            },
        )
        .map_err(|error| crate::Error::Config(error.to_string()))?;
    driver
        .finish_provider(
            attempt,
            "Widget fixed.",
            "",
            Usage {
                input: 1,
                output: 1,
                cached: 0,
                millis: 1,
            },
        )
        .map_err(|error| crate::Error::Config(error.to_string()))?;
    driver
        .close()
        .map_err(|error| crate::Error::Config(error.to_string()))?;
    drop(driver);

    catalog::set_name(&path, "widget spike")?;
    assert_eq!(catalog::name_of(&path).as_deref(), Some("widget spike"));
    assert!(catalog::set_name(&path, "  ").is_err());
    let listed = catalog::list_in(home.path())?;
    assert_eq!(listed[0].name.as_deref(), Some("widget spike"));

    let text = catalog::export(&path)?;
    assert!(text.starts_with("# session "), "{text}");
    assert!(text.contains("name: widget spike"), "{text}");
    assert!(text.contains("## You\n\nfix the widget\n\n"), "{text}");
    assert!(!text.contains("Harness state"), "{text}");
    assert!(text.contains("## Assistant\n\nWidget fixed."), "{text}");
    Ok(())
}
