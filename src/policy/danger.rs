use std::path::Path;

use super::{Block, builtin_deny, split_segments, unquoted, write_block};

pub(crate) fn danger(command: &str, workspace: &Path) -> Option<&'static str> {
    if builtin_deny(command) {
        return Some("it can damage the system");
    }
    if matches!(
        write_block(command.trim(), workspace, &[]),
        Some(Block::Protected(_))
    ) {
        return Some("it writes a protected path");
    }
    let segments = split_segments(command);
    let moved = segments
        .iter()
        .any(|segment| matches!(verb(&argv(segment)).as_deref(), Some("cd" | "pushd")));
    segments
        .iter()
        .find_map(|segment| risky(segment, workspace, moved))
}

pub(crate) fn argv(segment: &str) -> Vec<String> {
    unquoted(segment, &[" ", "\t"])
        .into_iter()
        .map(|word| word.trim_matches(|c| c == '"' || c == '\'').to_string())
        .filter(|word| !word.is_empty())
        .skip_while(|word| word.contains('=') && !word.starts_with('-'))
        .collect()
}

pub(crate) fn verb(words: &[String]) -> Option<String> {
    Some(words.first()?.rsplit('/').next()?.to_ascii_lowercase())
}

fn risky(segment: &str, workspace: &Path, moved: bool) -> Option<&'static str> {
    let words = argv(segment);
    let verb = verb(&words)?;
    let args = &words[1..];
    let lower = segment.to_ascii_lowercase();
    if lower.contains("drop database") || lower.contains("drop table") {
        return Some("it drops database objects");
    }
    match verb.as_str() {
        "rm" if args.iter().any(|arg| recursive(arg)) && !contained(args, workspace, moved) => {
            Some("it deletes recursively outside the workspace")
        }
        "find" if args.iter().any(|arg| arg == "-delete") => Some("it deletes files"),
        "sed"
            if args
                .iter()
                .any(|arg| arg.starts_with("-i") || arg.starts_with("--in-place"))
                && !contained(args, workspace, moved) =>
        {
            Some("it edits files outside the workspace in place")
        }
        "shred" => Some("it destroys file contents"),
        "git" => git(args),
        "terraform" | "tofu" if args.iter().any(|arg| arg == "destroy") => {
            Some("it destroys infrastructure")
        }
        "kubectl" if args.first().is_some_and(|arg| arg == "delete") => {
            Some("it deletes cluster resources")
        }
        "docker" | "podman" if args.iter().any(|arg| arg == "prune") => {
            Some("it deletes container data")
        }
        _other => None,
    }
}

fn contained(args: &[String], workspace: &Path, moved: bool) -> bool {
    let targets: Vec<&String> = args.iter().filter(|arg| !arg.starts_with('-')).collect();
    !targets.is_empty()
        && targets.iter().all(|target| {
            let path = Path::new(target.as_str());
            let inside = if path.is_absolute() {
                path.starts_with(workspace) && path != workspace
            } else {
                !moved && !matches!(target.as_str(), "." | "*" | "./" | "./*")
            };
            inside
                && !target.contains("..")
                && !target.contains('$')
                && !target.starts_with('~')
                && !target.contains('`')
        })
}

fn recursive(arg: &str) -> bool {
    arg == "--recursive"
        || (arg.starts_with('-') && !arg.starts_with("--") && arg.contains(['r', 'R']))
}

fn git(args: &[String]) -> Option<&'static str> {
    let mut rest = args.iter();
    let command = loop {
        let word = rest.next()?;
        if matches!(word.as_str(), "-C" | "-c") {
            rest.next();
        } else if !word.starts_with('-') {
            break word.as_str();
        }
    };
    let rest: Vec<&str> = rest.map(String::as_str).collect();
    let has = |flag: &str| rest.contains(&flag);
    match command {
        "reset" if has("--hard") => Some("it discards uncommitted changes"),
        "clean"
            if has("--force")
                || rest.iter().any(|arg| {
                    arg.starts_with('-') && !arg.starts_with("--") && arg.contains('f')
                }) =>
        {
            Some("it deletes untracked files")
        }
        "checkout" | "restore" if has(".") => Some("it discards uncommitted changes"),
        "branch" if has("-D") || (has("--delete") && has("--force")) => {
            Some("it force-deletes a branch")
        }
        "push"
            if has("--force")
                || has("-f")
                || has("--force-with-lease")
                || has("--delete")
                || has("--mirror")
                || rest
                    .iter()
                    .any(|arg| arg.starts_with('+') || arg.starts_with(':')) =>
        {
            Some("it rewrites or deletes remote history")
        }
        "stash" if has("drop") || has("clear") => Some("it deletes stashed work"),
        "filter-branch" | "filter-repo" => Some("it rewrites history"),
        _other => None,
    }
}
