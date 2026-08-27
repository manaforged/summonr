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
