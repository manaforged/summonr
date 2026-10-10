use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use super::{Tree, checkout_dir, new_id};
use crate::diff;
use crate::error::{Error, Result};

const SHARED: &[&str] = &["target", "node_modules"];
const LINK_LISTS: &[&str] = &[".summonr/linked", ".agents/linked"];

pub(super) fn open(primary: &Path) -> Result<Tree> {
    let primary = primary
        .canonicalize()
        .unwrap_or_else(|_| primary.to_path_buf());
    let head = diff::git_env(&primary, &[], &["rev-parse", "HEAD"])?;
    let objects = objects_dir(&primary)?;
    let id = new_id();
    let path = checkout_dir(&primary, "deltas", &id)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::create_dir_all(&path)?;
    let built = build(&primary, &path, &objects, &head, &id);
    if built.is_err() {
        drop(fs::remove_dir_all(&path));
    }
    built
}

fn build(primary: &Path, path: &Path, objects: &Path, head: &str, id: &str) -> Result<Tree> {
    diff::git_env(path, &[], &["init", "-q"])?;
    let info = path.join(".git/objects/info");
    fs::create_dir_all(&info)?;
    fs::write(info.join("alternates"), format!("{}\n", objects.display()))?;
    let names = link_names(primary);
    exclude(path, &names)?;
    diff::git_env(
        path,
        &[],
        &[
            "-c",
            "advice.detachedHead=false",
            "checkout",
            "--detach",
            head,
        ],
    )?;
    overlay(primary, path, &names)?;
    share(primary, path, &names)?;
    diff::git_env(path, &[], &["add", "-A"])?;
    let snapshot = diff::git_env(path, &[], &["write-tree"])?;
    seal(path, head, &snapshot)?;
    Ok(Tree {
        primary: primary.to_path_buf(),
        path: path.to_path_buf(),
        branch: format!("summonr/{id}"),
        base: head.to_string(),
        snapshot: Some(snapshot),
    })
}

fn seal(path: &Path, head: &str, snapshot: &str) -> Result<()> {
    let spec = format!("{head}^{{tree}}");
    let head_tree = diff::git_env(path, &[], &["rev-parse", &spec])?;
    if head_tree == snapshot {
        return Ok(());
    }
    let commit = diff::git_env(
        path,
        &[
            ("GIT_AUTHOR_NAME", OsStr::new("summonr")),
            ("GIT_AUTHOR_EMAIL", OsStr::new("summonr@localhost")),
            ("GIT_COMMITTER_NAME", OsStr::new("summonr")),
            ("GIT_COMMITTER_EMAIL", OsStr::new("summonr@localhost")),
        ],
        &["commit-tree", snapshot, "-p", head, "-m", "summonr delta"],
    )?;
    diff::git_env(path, &[], &["update-ref", "HEAD", &commit])?;
    Ok(())
}

fn objects_dir(primary: &Path) -> Result<PathBuf> {
    let text = diff::git_env(primary, &[], &["rev-parse", "--git-common-dir"])?;
    let common = PathBuf::from(&text);
    let common = if common.is_absolute() {
        common
    } else {
        primary.join(common)
    };
    Ok(common.join("objects"))
}

fn link_names(primary: &Path) -> Vec<String> {
    let mut names: Vec<String> = SHARED.iter().map(|name| (*name).to_string()).collect();
    for list in LINK_LISTS {
        let Ok(text) = fs::read_to_string(primary.join(list)) else {
            continue;
        };
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            names.push(line.to_string());
        }
    }
    names.sort();
    names.dedup();
    names
}

fn exclude(dest: &Path, names: &[String]) -> Result<()> {
    let path = dest.join(".git/info/exclude");
    let mut text = fs::read_to_string(&path).unwrap_or_default();
    if !text.ends_with('\n') && !text.is_empty() {
        text.push('\n');
    }
    for name in names.iter().cloned().chain(diff::ignored::patterns()) {
        text.push_str(&name);
        text.push('\n');
    }
    fs::write(path, text)?;
    Ok(())
}

fn overlay(primary: &Path, dest: &Path, names: &[String]) -> Result<()> {
    let patch = diff::git_output(primary, &[], &["diff", "--binary", "HEAD"], None)?;
    if !patch.is_empty() {
        diff::git_output(
            dest,
            &[],
            &["apply", "--binary", "--whitespace=nowarn", "-"],
            Some(&patch),
        )?;
    }
    let listed = diff::git_paths(
        primary,
        &["ls-files", "-z", "--others", "--exclude-standard"],
    )?;
    let skip: HashSet<&str> = names.iter().map(String::as_str).collect();
    for line in listed.iter().map(String::as_str) {
        let head = line.split(['/', '\\']).next().unwrap_or(line);
        if skip.contains(head) {
            continue;
        }
        copy_file(&primary.join(line), &dest.join(line))?;
    }
    Ok(())
}

fn share(primary: &Path, dest: &Path, names: &[String]) -> Result<()> {
    for name in names {
        let from = primary.join(name);
        if !from.exists() {
            continue;
        }
        let to = dest.join(name);
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)?;
        }
        if to.symlink_metadata().is_ok() {
            if to.is_dir() && !to.is_symlink() {
                fs::remove_dir_all(&to)?;
            } else {
                fs::remove_file(&to)?;
            }
        }
        link(&from, &to).map_err(|error| {
            Error::Config(format!(
                "cannot share {} with the delta: {error}",
                from.display()
            ))
        })?;
    }
    Ok(())
}

fn copy_file(from: &Path, to: &Path) -> Result<()> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(from, to)?;
    Ok(())
}

#[cfg(unix)]
fn link(from: &Path, to: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(from, to)
}

#[cfg(windows)]
fn link(from: &Path, to: &Path) -> std::io::Result<()> {
    if from.is_dir() {
        std::os::windows::fs::symlink_dir(from, to)
    } else {
        std::os::windows::fs::symlink_file(from, to)
    }
}
