use super::*;

pub(super) fn fresh(
    home: Result<PathBuf>,
    workspace: &Path,
    model: &str,
    system: &str,
) -> Result<SessionDriver> {
    let failure = match home.and_then(|home| open_driver(home, workspace, model, system)) {
        Ok(session) => return Ok(session),
        Err(error) => error,
    };
    let ephemeral = std::env::temp_dir().join(format!("summonr-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&ephemeral));
    if std::fs::create_dir_all(&ephemeral).is_err() {
        return Err(failure);
    }
    let session = open_driver(&ephemeral, workspace, model, system)?;
    eprintln!(
        "summonr: the summonr home is not writable, so session {} lives in {} and will not resume",
        session.id(),
        ephemeral.display()
    );
    Ok(session)
}

#[cfg(all(test, unix))]
#[path = "session_home_tests.rs"]
mod tests;
