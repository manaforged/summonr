use super::*;
use crate::error::Result;

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
            .start_input(
                "fix the widget\n\nHarness state: implementation mode is active.",
                &[],
            )
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
