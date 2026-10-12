use super::*;

fn paths(dir: &Path, exclude: Option<&Path>) -> io::Result<Vec<PathBuf>> {
    let mut paths = match fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(std::result::Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "jsonl")
            })
            .filter(|path| exclude != Some(path.as_path()))
            .collect::<Vec<_>>(),
        Err(error) if error.kind() == ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(error),
    };
    paths.sort();
    Ok(paths)
}

pub(super) fn runs(dir: &Path, exclude: Option<&Path>) -> io::Result<Vec<RunInfo>> {
    paths(dir, exclude)?
        .into_iter()
        .map(|path| {
            Ok(RunInfo {
                status: read_status(&path)?,
                path,
            })
        })
        .collect()
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Event {
    Panic,
    Exit,
    Session,
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
struct Record {
    event: Option<Event>,
    code: Option<i64>,
}

fn read_status(path: &Path) -> io::Result<RunStatus> {
    Ok(status(path, &fs::read(path)?))
}

fn status(path: &Path, bytes: &[u8]) -> RunStatus {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return RunStatus::Incomplete;
    };
    let mut panic = false;
    let mut exit = None;
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let Ok(record) = serde_json::from_str::<Record>(line) else {
            return RunStatus::Incomplete;
        };
        match record.event {
            Some(Event::Panic) => panic = true,
            Some(Event::Exit) => exit = record.code,
            Some(Event::Session) => exit = None,
            Some(Event::Other) | None => {}
        }
    }
    if panic {
        RunStatus::Panic
    } else if exit == Some(0) {
        RunStatus::Complete
    } else if exit.is_some() {
        RunStatus::Failed
    } else if active(path) {
        RunStatus::Active
    } else {
        RunStatus::Incomplete
    }
}

fn active(path: &Path) -> bool {
    let Ok(lock) = OpenOptions::new()
        .read(true)
        .write(true)
        .open(lock_path(path))
    else {
        return false;
    };
    lock.try_lock().is_err()
}

pub(super) fn lock_path(path: &Path) -> PathBuf {
    let mut path = path.as_os_str().to_os_string();
    path.push(OsString::from(".lock"));
    PathBuf::from(path)
}

pub(super) fn sweep(dir: &Path) -> Option<String> {
    let paths = paths(dir, None).ok()?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok();
    let mut kept = Vec::with_capacity(paths.len());
    for path in paths {
        let age = fs::metadata(&path)
            .and_then(|metadata| metadata.modified())
            .and_then(|modified| {
                modified
                    .duration_since(UNIX_EPOCH)
                    .map_err(io::Error::other)
            });
        let expired = match (now, age) {
            (Some(now), Ok(age)) => now.saturating_sub(age) > summonr::limits::log_retain(),
            _ => false,
        };
        if !expired {
            kept.push((path, None));
            continue;
        }
        match read_status(&path) {
            Ok(RunStatus::Active) => kept.push((path, Some(RunStatus::Active))),
            Ok(_) => {
                drop(fs::remove_file(&path));
                drop(fs::remove_file(lock_path(&path)));
            }
            Err(_) => kept.push((path, None)),
        }
    }
    scan(dir, kept)
}

fn scan(dir: &Path, kept: Vec<(PathBuf, Option<RunStatus>)>) -> Option<String> {
    let marker = dir.join("crash-scanned");
    let scanned = fs::read_to_string(&marker).unwrap_or_default();
    let scanned = scanned.trim();
    let mut latest = None;
    let mut advance = None;
    let mut blocked = false;
    for (path, known) in kept {
        let Some(name) = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
        else {
            continue;
        };
        if name.as_str() <= scanned {
            continue;
        }
        let status = match known {
            Some(status) => Some(status),
            None => read_status(&path).ok(),
        };
        match status {
            Some(RunStatus::Incomplete | RunStatus::Panic) => latest = Some(name.clone()),
            Some(RunStatus::Active) | None => blocked = true,
            Some(RunStatus::Complete | RunStatus::Failed) => {}
        }
        if !blocked {
            advance = Some(name);
        }
    }
    if let Some(name) = advance {
        drop(fs::write(&marker, name));
    }
    latest
}
