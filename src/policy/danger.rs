use std::path::{Component, Path, PathBuf};

use super::{
    Block, Cwd, Hazard, TEXT_HAZARDS, any_hit, builtin_deny, split_segments, subcommand, unquoted,
    verb_row, write_block_segments,
};

pub(crate) fn danger(command: &str, workspace: &Path) -> Option<&'static str> {
    let command = &without_heredocs(command);
    let segments = split_segments(command.trim());
    if builtin_deny(command, &segments) {
        return Some("it can damage the system");
    }
    if matches!(
        write_block_segments(&segments, workspace, &[]),
        Some(Block::Protected(_))
    ) {
        return Some("it writes a protected path");
    }
    let mut cwd = Some(workspace.to_path_buf());
    segments.iter().find_map(|segment| {
        let reason = risky(segment, workspace, cwd.as_deref());
        cwd = next_cwd(segment, cwd.take());
        reason
    })
}

fn without_heredocs(command: &str) -> String {
    let mut kept = Vec::new();
    let mut ending: Option<String> = None;
    for line in command.lines() {
        if let Some(end) = &ending {
            if line.trim() == end {
                ending = None;
            }
            continue;
        }
        kept.push(line);
        ending = heredoc_end(line);
    }
    kept.join("\n")
}

fn heredoc_end(line: &str) -> Option<String> {
    let (_, rest) = line.split_once("<<")?;
    let rest = rest.trim_start_matches(['-', '~']).trim_start();
    if rest.starts_with('<') {
        return None;
    }
    let word: String = rest
        .trim_start_matches(['\'', '"'])
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    (!word.is_empty()).then_some(word)
}

fn next_cwd(segment: &str, cwd: Option<PathBuf>) -> Option<PathBuf> {
    let words = argv(segment);
    let row = verb(&words).and_then(|verb| verb_row(&verb));
    match row.map(|row| row.cwd) {
        Some(Cwd::Tracked) => {
            let target = words.iter().skip(1).find(|word| !word.starts_with('-'))?;
            resolve(target, cwd.as_deref()?)
        }
        Some(Cwd::Changes) => None,
        Some(Cwd::Stays) | None => cwd,
    }
}

fn resolve(target: &str, cwd: &Path) -> Option<PathBuf> {
    if target.contains(['$', '`']) || target.starts_with('~') {
        return None;
    }
    let mut path = if Path::new(target).is_absolute() {
        PathBuf::new()
    } else {
        cwd.to_path_buf()
    };
    for part in Path::new(target).components() {
        match part {
            Component::ParentDir => {
                path.pop();
            }
            Component::CurDir => {}
            other => path.push(other),
        }
    }
    Some(path)
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

fn risky(segment: &str, workspace: &Path, cwd: Option<&Path>) -> Option<&'static str> {
    let words = argv(segment);
    let verb = verb(&words)?;
    let args = &words[1..];
    let lower = segment.to_ascii_lowercase();
    if let Some((_, reason)) = TEXT_HAZARDS.iter().find(|(text, _)| lower.contains(text)) {
        return Some(reason);
    }
    match verb_row(&verb)?.hazard {
        Hazard::None => None,
        Hazard::Args(rules) => rules
            .iter()
            .find(|rule| any_hit(rule.when, args) && !(rule.broad && narrow(args, workspace, cwd)))
            .map(|rule| rule.reason),
        Hazard::Sub { skip, rules } => {
            let (sub, rest) = subcommand(args, skip)?;
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
    let path = if path
        .file_name()
        .is_some_and(|name| name.to_string_lossy().contains('*'))
    {
        path.parent().unwrap_or(path)
    } else {
        path
    };
    let parts: Vec<String> = path
        .components()
        .filter_map(|part| match part {
            Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    let shallow = match parts.as_slice() {
        [] => true,
        [first] => ROOTS.contains(&first.as_str()) || !path.is_absolute(),
        [first, _] => matches!(
            first.as_str(),
            "usr" | "etc" | "var" | "opt" | "System" | "Library" | "private"
        ),
        _ => false,
    };
    let guarded = |root: &Path| root.starts_with(path);
    shallow || guarded(workspace) || home.is_some_and(guarded)
}
