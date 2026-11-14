use std::io::ErrorKind;
use std::sync::OnceLock;

use super::*;

pub(super) fn fresh(
    home: Result<PathBuf>,
    workspace: &Path,
    model: &str,
    system: &str,
) -> Result<SessionDriver> {
    let (failure, read_only) = match home {
        Ok(home) => match open_driver(&home, workspace, model, system) {
            Ok(session) => return Ok(session),
            Err(error) => (error, unwritable(&home)),
        },
        Err(Error::Io(error)) => {
            let read_only = denied(error.kind());
            (Error::Io(error), read_only)
        }
        Err(error) => return Err(error),
    };
    let Some(ephemeral) = read_only.then(ephemeral).flatten() else {
        return Err(failure);
    };
    match open_driver(ephemeral, workspace, model, system) {
        Ok(session) => {
            eprintln!(
                "summonr: the summonr home is not writable, so session {} lives in {} and will not resume",
                session.id(),
                ephemeral.display()
            );
            Ok(session)
        }
        Err(_) => Err(failure),
    }
}

fn denied(kind: ErrorKind) -> bool {
    matches!(
        kind,
        ErrorKind::PermissionDenied | ErrorKind::ReadOnlyFilesystem
    )
}

fn unwritable(home: &Path) -> bool {
    let probe = home.join(format!(".probe-{}", uuid::Uuid::now_v7().simple()));
    match std::fs::File::create(&probe) {
        Ok(_) => {
            drop(std::fs::remove_file(&probe));
            false
        }
        Err(error) => denied(error.kind()),
    }
}

pub(super) fn ephemeral() -> Option<&'static Path> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("summonr-{}", uuid::Uuid::now_v7().simple()));
        #[cfg(unix)]
        let builder = {
            let mut builder = std::fs::DirBuilder::new();
            std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
            builder
        };
        #[cfg(not(unix))]
        let builder = std::fs::DirBuilder::new();
        builder.create(&dir).is_ok().then_some(dir)
    })
    .as_deref()
}

#[cfg(all(test, unix))]
#[path = "session_home_tests.rs"]
mod tests;
