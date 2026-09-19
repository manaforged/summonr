use super::*;

pub(in crate::tui::sigil_view) fn editor() -> String {
    std::env::var("VISUAL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| std::env::var("EDITOR").ok())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "vi".to_string())
}

pub(in crate::tui::sigil_view) fn edit(path: &Path, editor: &str) -> Result<()> {
    #[cfg(windows)]
    let mut command = {
        let mut command = Command::new("cmd.exe");
        command
            .args(["/D", "/V:OFF", "/S", "/C"])
            .env("SUMMONR_EDITOR_FILE", path)
            .raw_arg(format!("\"{editor} \"%SUMMONR_EDITOR_FILE%\"\""));
        command
    };
    #[cfg(not(windows))]
    let mut command = {
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg(format!("exec {editor} \"$1\""))
            .arg("summonr-editor")
            .arg(path);
        command
    };
    let status = command
        .status()
        .map_err(|error| Error::Tool(format!("cannot start {editor}: {error}")))?;
    if !status.success() {
        return Err(Error::Tool(format!("editor exited with {status}")));
    }
    Ok(())
}

pub(in crate::tui::sigil_view) fn edit_external(initial: &str, editor: &str) -> Result<String> {
    let mut draft = tempfile::Builder::new().suffix(".md").tempfile()?;
    draft.write_all(initial.as_bytes())?;
    draft.flush()?;
    edit(draft.path(), editor)?;
    Ok(read_to_string(draft.path())?)
}
