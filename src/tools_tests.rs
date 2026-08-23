use super::*;

impl ToolRegistry {
    pub(crate) fn new(workspace: impl AsRef<Path>) -> Result<Self> {
        Self::with_policy_and_search(workspace, Policy::builtin(), None)
    }

    pub(crate) fn with_policy(workspace: impl AsRef<Path>, policy: Policy) -> Result<Self> {
        Self::with_policy_and_search(workspace, policy, None)
    }
}

#[test]
fn attachments_separate_images_from_text() -> Result<()> {
    let workspace = std::env::temp_dir().join(format!(
        "mage-attach-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| Error::Tool(error.to_string()))?
            .as_nanos()
    ));
    std::fs::create_dir_all(&workspace)?;
    std::fs::write(workspace.join("note.txt"), "hello")?;
    std::fs::write(workspace.join("pixel.png"), b"\x89PNG\r\n\x1a\n")?;

    let input = attach(&workspace, "compare @note.txt @pixel.png")?;
    assert!(input.text.starts_with("compare\n"));
    assert!(input.text.contains("--- note.txt ---\nhello"));
    assert_eq!(input.images.len(), 1);
    assert_eq!(input.images[0].media_type, "image/png");

    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[test]
fn attachments_reject_files_outside_the_workspace() -> Result<()> {
    let base = std::env::temp_dir().join(format!("mage-attach-boundary-{}", std::process::id()));
    let workspace = base.join("workspace");
    std::fs::create_dir_all(&workspace)?;
    std::fs::write(base.join("outside.png"), b"\x89PNG\r\n\x1a\n")?;

    let error = attach(&workspace, "look @../outside.png")
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();
    assert!(error.contains("outside the workspace"));

    std::fs::remove_dir_all(base)?;
    Ok(())
}

#[test]
fn attachments_limit_total_image_bytes() -> Result<()> {
    let workspace = std::env::temp_dir().join(format!("mage-attach-size-{}", std::process::id()));
    std::fs::create_dir_all(&workspace)?;
    let mut image = vec![0_u8; 6 * 1024 * 1024];
    image[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
    std::fs::write(workspace.join("one.png"), &image)?;
    std::fs::write(workspace.join("two.png"), &image)?;

    let error = attach(&workspace, "@one.png @two.png")
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();
    assert!(error.contains("attached images are larger"));

    std::fs::remove_dir_all(workspace)?;
    Ok(())
}
