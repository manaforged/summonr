use super::*;
use crate::error::Result;

#[test]
fn format_2_session_matches_its_workspace_identity() -> Result<()> {
    let home = tempfile::tempdir()?;
    let workspace = home.path().join("workspace");
    std::fs::create_dir(&workspace)?;
    let driver = crate::durable::product::open(&crate::durable::product::Open {
        home: home.path().to_path_buf(),
        workspace: workspace.clone(),
        model: "test-model".to_string(),
        system: "test".to_string(),
    })
    .map_err(|error| crate::Error::Config(error.to_string()))?;
    let path = driver.path().to_path_buf();
    drop(driver);

    assert!(super::catalog::format2_workspace_matches(
        &path,
        &[workspace.display().to_string()]
    ));
    let other = home.path().join("other");
    std::fs::create_dir(&other)?;
    assert!(!super::catalog::format2_workspace_matches(
        &path,
        &[other.display().to_string()]
    ));
    Ok(())
}

#[test]
fn harness_text_follows_the_user_text() {
    assert_eq!(decorate("control", "request"), "request\n\ncontrol");
    assert_eq!(decorate("control", ""), "control");
}
