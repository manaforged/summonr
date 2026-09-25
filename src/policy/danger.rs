use std::path::Path;

use super::{
    Block, Cwd, Hazard, TEXT_HAZARDS, any_hit, builtin_deny, split_segments, subcommand, unquoted,
    verb_row, write_block_segments,
};

pub(crate) fn danger(command: &str, workspace: &Path) -> Option<&'static str> {
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
    let moved = segments.iter().any(|segment| {
        verb(&argv(segment))
            .and_then(|verb| verb_row(&verb))
            .is_some_and(|row| row.cwd != Cwd::Stays)
    });
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
    if let Some((_, reason)) = TEXT_HAZARDS.iter().find(|(text, _)| lower.contains(text)) {
        return Some(reason);
    }
    match verb_row(&verb)?.hazard {
        Hazard::None => None,
        Hazard::Args(rules) => rules
            .iter()
            .find(|rule| {
                any_hit(rule.when, args) && !(rule.outside && contained(args, workspace, moved))
            })
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
