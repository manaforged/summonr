use std::path::{Component, Path, PathBuf};

use super::walk::{Step, resolve, walk, without_heredocs};
use super::{
    Block, Hazard, INQUIRY_FLAGS, TEXT_HAZARDS, any_hit, builtin_deny, subcommand, verb_row,
    write_block,
};

pub(crate) fn danger(command: &str, workspace: &Path) -> Option<&'static str> {
    let steps = walk(command, workspace, &[]);
    let texts: Vec<String> = steps.iter().map(|step| step.text.clone()).collect();
    if builtin_deny(&without_heredocs(command), &texts) {
        return Some("it can damage the system");
    }
    if matches!(
        write_block(&steps, workspace, &[]),
        Some(Block::Protected(_))
    ) {
        return Some("it writes a protected path");
    }
    steps.iter().find_map(|step| risky(step, workspace))
}

fn inquiry(args: &[String]) -> bool {
    let asked: Vec<&String> = args
        .iter()
        .filter(|arg| {
            !arg.trim_start_matches(|c: char| c.is_ascii_digit())
                .starts_with(['>', '<'])
        })
        .collect();
    !asked.is_empty()
        && asked
            .iter()
            .all(|arg| INQUIRY_FLAGS.contains(&arg.as_str()))
}

fn risky(step: &Step, workspace: &Path) -> Option<&'static str> {
    let verb = step.verb()?;
    let args = &step.argv()[1..];
    let cwd = step.cwd.as_deref();
    let lower = step.text.to_ascii_lowercase();
    if let Some((_, reason)) = TEXT_HAZARDS.iter().find(|(text, _)| lower.contains(text)) {
        return Some(reason);
    }
    match verb_row(&verb)?.hazard {
        Hazard::None => None,
        Hazard::Args(_) if inquiry(args) => None,
        Hazard::Args(rules) => rules
            .iter()
            .find(|rule| any_hit(rule.when, args) && !(rule.broad && narrow(args, workspace, cwd)))
            .map(|rule| rule.reason),
        Hazard::Sub { skip, rules } => {
            let (sub, rest) = subcommand(args, skip)?;
            if inquiry(rest) {
                return None;
            }
            rules
                .iter()
                .find(|rule| rule.names.contains(&sub) && any_hit(rule.when, rest))
                .map(|rule| rule.reason)
        }
    }
}

const ROOTS: &[&str] = &[
    "bin",
    "boot",
    "dev",
    "etc",
    "home",
    "lib",
    "lib32",
    "lib64",
    "opt",
    "proc",
    "root",
    "run",
    "sbin",
    "srv",
    "sys",
    "usr",
    "var",
    "tmp",
    "Applications",
    "Library",
    "System",
    "Users",
    "Volumes",
    "private",
];

fn narrow(args: &[String], workspace: &Path, cwd: Option<&Path>) -> bool {
    let targets: Vec<&String> = args.iter().filter(|arg| !arg.starts_with('-')).collect();
    let home = std::env::var_os("HOME").map(PathBuf::from);
    !targets.is_empty()
        && targets.iter().all(|target| {
            cwd.and_then(|cwd| resolve(target, cwd))
                .map(|path| std::fs::canonicalize(&path).unwrap_or(path))
                .is_some_and(|path| !broad(&path, workspace, home.as_deref()))
        })
}

fn broad(path: &Path, workspace: &Path, home: Option<&Path>) -> bool {
    let path = if path.file_name().is_some_and(|name| {
        name.to_string_lossy()
            .chars()
            .all(|c| matches!(c, '*' | '?' | '.'))
    }) {
        path.parent().unwrap_or(path)
    } else {
        path
    };
    let parts: Vec<String> = path
        .components()
        .filter_map(|part| match part {
            Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
            Component::Prefix(_)
            | Component::RootDir
            | Component::CurDir
            | Component::ParentDir => None,
        })
        .collect();
    let shallow = match parts.as_slice() {
        [] => true,
        [first] => ROOTS.contains(&first.as_str()) || !path.is_absolute(),
        [first, _] => matches!(
            first.as_str(),
            "usr" | "etc" | "var" | "opt" | "System" | "Library" | "private"
        ),
        [_, _, _, ..] => false,
    };
    let guarded = |root: &Path| root.starts_with(path);
    shallow || guarded(workspace) || home.is_some_and(guarded)
}
