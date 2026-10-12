use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};

use ignore::WalkBuilder;

const MAGIC: &str = "summonr-names 1";
pub(crate) const ENTRY: &str = "--index-files";
const STALE_TEMP_SECS: u64 = 3_600;

pub(super) struct Index {
    file: PathBuf,
    pub(super) root: PathBuf,
    pub(super) built: u64,
}

#[cfg(test)]
fn folder() -> Option<PathBuf> {
    None
}

#[cfg(not(test))]
fn folder() -> Option<PathBuf> {
    crate::prompt::summonr_home()
        .ok()
        .map(|home| home.join("cache").join("names"))
}

fn file_for(root: &Path) -> Option<PathBuf> {
    let key = crate::text::fnv64(root.to_string_lossy().as_bytes());
    folder().map(|dir| dir.join(format!("{key}.idx")))
}

fn header(file: &Path) -> Option<(PathBuf, u64)> {
    let mut reader = BufReader::new(File::open(file).ok()?);
    let mut lines = [String::new(), String::new(), String::new()];
    for line in &mut lines {
        reader.read_line(line).ok()?;
    }
    (lines[0].trim_end() == MAGIC).then_some(())?;
    Some((
        PathBuf::from(lines[1].trim_end()),
        lines[2].trim_end().parse().ok()?,
    ))
}

pub(super) fn covering(root: &Path) -> Option<Index> {
    root.ancestors().find_map(|ancestor| {
        let file = file_for(ancestor)?;
        let (indexed, built) = header(&file)?;
        (indexed == ancestor).then_some(Index {
            file,
            root: indexed,
            built,
        })
    })
}

pub(super) fn each(
    index: &Index,
    root: &Path,
    words: &[String],
    mut visit: impl FnMut(PathBuf) -> bool,
) {
    let Ok(file) = File::open(&index.file) else {
        return;
    };
    let mut reader = BufReader::new(file);
    for _ in 0..3 {
        let mut skip = String::new();
        if reader.read_line(&mut skip).is_err() {
            return;
        }
    }
    let prefix = root.to_string_lossy().into_owned();
    let prefix = prefix.trim_end_matches('/').as_bytes().to_vec();
    let words: Vec<&[u8]> = words.iter().map(String::as_bytes).collect();
    let mut entry: Vec<u8> = Vec::new();
    let mut inside = false;
    while let (Some(shared), Some(len)) = (number(&mut reader), number(&mut reader)) {
        entry.truncate(shared);
        let start = entry.len();
        entry.resize(start + len, 0);
        if reader.read_exact(&mut entry[start..]).is_err() {
            return;
        }
        if !under(&entry, &prefix) {
            if inside {
                return;
            }
            continue;
        }
        inside = true;
        if !named(&entry, &words) {
            continue;
        }
        let path = PathBuf::from(String::from_utf8_lossy(&entry).into_owned());
        if !visit(path) {
            return;
        }
    }
}

fn under(entry: &[u8], prefix: &[u8]) -> bool {
    entry.starts_with(prefix)
        && (prefix.is_empty() || entry.get(prefix.len()).is_none_or(|next| *next == b'/'))
}

fn named(entry: &[u8], words: &[&[u8]]) -> bool {
    if words.is_empty() {
        return true;
    }
    let name = entry
        .iter()
        .rposition(|byte| *byte == b'/')
        .map_or(entry, |slash| &entry[slash + 1..]);
    words.iter().all(|word| {
        !word.is_ascii()
            || name
                .windows(word.len().max(1))
                .any(|window| window.eq_ignore_ascii_case(word))
    })
}

fn number(reader: &mut impl Read) -> Option<usize> {
    let mut bytes = [0_u8; 4];
    reader.read_exact(&mut bytes).ok()?;
    usize::try_from(u32::from_le_bytes(bytes)).ok()
}

fn building() -> &'static Mutex<HashSet<PathBuf>> {
    static BUILDING: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
    BUILDING.get_or_init(|| Mutex::new(HashSet::new()))
}

pub(super) fn refresh(root: &Path, slow: bool) {
    let covered = covering(root);
    if covered.is_none() && !slow {
        return;
    }
    let fresh = covered.as_ref().is_some_and(|index| {
        crate::mail::now().saturating_sub(index.built) < crate::limits::limits().search_index_secs
    });
    let root = covered.map_or_else(|| root.to_path_buf(), |index| index.root);
    if fresh
        || !building()
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(root.clone())
    {
        return;
    }
    if in_progress(&root) || spawn(&root).is_err() {
        building()
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&root);
    }
}

fn spawn(root: &Path) -> std::io::Result<()> {
    let mut command = std::process::Command::new(std::env::current_exe()?);
    command
        .arg(ENTRY)
        .arg(root)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    let mut child = command.spawn()?;
    let root = root.to_path_buf();
    std::thread::Builder::new()
        .name("search-index".to_string())
        .spawn(move || {
            drop(child.wait());
            building()
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&root);
        })?;
    Ok(())
}

fn in_progress(root: &Path) -> bool {
    let Some(target) = file_for(root) else {
        return false;
    };
    let Some(Ok(entries)) = target.parent().map(std::fs::read_dir) else {
        return false;
    };
    let prefix = format!(
        "{}.",
        target.file_stem().unwrap_or_default().to_string_lossy()
    );
    entries.filter_map(Result::ok).any(|entry| {
        let name = entry.file_name().to_string_lossy().into_owned();
        name.starts_with(&prefix) && name.ends_with(".tmp") && !aged(&entry)
    })
}

fn aged(entry: &std::fs::DirEntry) -> bool {
    entry
        .metadata()
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.elapsed().ok())
        .is_some_and(|age| age.as_secs() > STALE_TEMP_SECS)
}

pub(crate) fn build(root: &Path) -> std::io::Result<()> {
    let Some(target) = file_for(root) else {
        return Ok(());
    };
    let dir = target.parent().unwrap_or(root);
    std::fs::create_dir_all(dir)?;
    sweep(dir);
    let temp = target.with_extension(format!("{}.tmp", std::process::id()));
    let mut out = BufWriter::new(File::create(&temp)?);
    writeln!(
        out,
        "{MAGIC}\n{}\n{}",
        root.to_string_lossy(),
        crate::mail::now()
    )?;
    let mut last: Vec<u8> = Vec::new();
    let walk = WalkBuilder::new(root)
        .standard_filters(false)
        .sort_by_file_name(std::ffi::OsStr::cmp)
        .filter_entry(|entry| {
            entry.depth() == 0
                || !entry.file_type().is_some_and(|kind| kind.is_dir())
                || !(crate::STATE_DIRS.contains(&entry.file_name().to_string_lossy().as_ref())
                    || super::walk::skipped_root(entry.path())
                    || crate::policy::protected_read(entry.path()))
        })
        .build();
    for entry in walk.filter_map(Result::ok) {
        let listed = entry
            .file_type()
            .is_some_and(|kind| kind.is_file() || (kind.is_symlink() && !entry.path().is_dir()));
        if !listed {
            continue;
        }
        let text = entry.path().to_string_lossy();
        let bytes = text.as_bytes();
        let shared = last
            .iter()
            .zip(bytes)
            .take_while(|(left, right)| left == right)
            .count();
        let (Ok(shared_len), Ok(rest_len)) =
            (u32::try_from(shared), u32::try_from(bytes.len() - shared))
        else {
            continue;
        };
        out.write_all(&shared_len.to_le_bytes())?;
        out.write_all(&rest_len.to_le_bytes())?;
        out.write_all(&bytes[shared..])?;
        last.clear();
        last.extend_from_slice(bytes);
    }
    out.flush()?;
    drop(out);
    std::fs::rename(&temp, &target)
}

fn sweep(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "tmp") && aged(&entry) {
            drop(std::fs::remove_file(path));
        }
    }
}
