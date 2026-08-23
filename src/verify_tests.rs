use super::*;

#[test]
fn check_commands_read_workspace_toml() -> Result<()> {
    let dir = std::env::temp_dir().join(format!("mage-check-{}", std::process::id()));
    std::fs::create_dir_all(dir.join(".mage"))?;
    assert_eq!(check_commands(&dir)?, None);
    std::fs::write(dir.join(".mage/check.toml"), "check = \"cargo test\"")?;
    assert_eq!(check_commands(&dir)?, Some(vec!["cargo test".to_string()]));
    std::fs::write(
        dir.join(".mage/check.toml"),
        "checks = [\"cargo clippy\", \"cargo test\"]",
    )?;
    assert_eq!(
        check_commands(&dir)?,
        Some(vec!["cargo clippy".to_string(), "cargo test".to_string()])
    );
    std::fs::write(
        dir.join(".mage/check.toml"),
        "check = \"a\"\nchecks = [\"b\"]",
    )?;
    check_commands(&dir).unwrap_err();
    std::fs::write(dir.join(".mage/check.toml"), "check = \"  \"")?;
    check_commands(&dir).unwrap_err();
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}
