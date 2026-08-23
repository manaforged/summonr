use super::*;

#[test]
fn catalog_and_load_from_workspace() -> Result<()> {
    let dir = std::env::temp_dir().join(format!(
        "mage-skills-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| Error::Config(error.to_string()))?
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join(".mage/skills/commit"))?;
    std::fs::write(
        dir.join(".mage/skills/commit/SKILL.md"),
        "# Git commit\n\nWrite a Tim Pope message.\n",
    )?;
    let listed = catalog(&dir).ok_or_else(|| Error::Tool("catalog missing".to_string()))?;
    assert!(listed.contains("commit"));
    assert!(listed.contains("Git commit"));
    let body = load(&dir, "commit")?;
    assert!(body.contains("Tim Pope"));
    load(&dir, "../etc").unwrap_err();
    std::fs::remove_dir_all(dir)?;
    Ok(())
}
