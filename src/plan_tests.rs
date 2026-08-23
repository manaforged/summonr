use super::*;

fn dir() -> crate::Result<PathBuf> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("mage-plan-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[test]
fn write_read_clear_roundtrip() -> crate::Result<()> {
    let workspace = dir()?;
    assert_eq!(read(&workspace), None);
    write(&workspace, "# Plan\n1. step\n")?;
    assert_eq!(read(&workspace).as_deref(), Some("# Plan\n1. step\n"));
    assert!(clear(&workspace)?);
    assert_eq!(read(&workspace), None);
    assert!(!clear(&workspace)?);
    std::fs::remove_dir_all(&workspace)?;
    Ok(())
}
