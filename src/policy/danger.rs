use std::path::{Component, Path};

use super::shell::{command_word, interpreter, shell};
use super::verbs::{Rule, SubRule};
use super::walk::{Step, heredocs, resolve, walk, without_heredocs};
use super::{
    Block, Gate, Hazard, any_hit, builtin_deny, catastrophic, rules, subcommand, unquoted,
    verb_row, write_block,
};

pub(crate) fn danger(
    command: &str,
    workspace: &Path,
    start: &Path,
    gate: Gate,
) -> Option<&'static str> {
    let steps = walk(command, workspace, start, &[]);
    let texts: Vec<String> = steps.iter().map(|step| step.text.clone()).collect();
    let flat = without_heredocs(command);
    let hit = match gate {
        Gate::Destructive => catastrophic(&flat),
        Gate::Risky => builtin_deny(&flat, &texts),
    };
    if hit {
        return Some("it can damage the system");
    }
    if let Some(reason) = heredocs(command)
        .iter()
        .filter(|(opening, _)| feeds_shell(opening))
        .find_map(|(_, body)| danger(body, workspace, start, gate))
    {
        return Some(reason);
    }
    if gate == Gate::Risky
        && matches!(
            write_block(&steps, workspace, &[]),
            Some(Block::Protected(_))
        )
    {
        return Some("it writes a protected path");
    }
    steps.iter().find_map(|step| risky(step, workspace, gate))
}

fn feeds_shell(opening: &str) -> bool {
    let head = opening.split_once("<<").map_or(opening, |(head, _)| head);
    unquoted(head, &["&&", "||", "|", ";", "&"])
        .last()
        .and_then(|segment| {
            command_word(segment, false)
                .map(|word| shell(word) || interpreter(word) || matches!(word, "source" | "."))
        })
        .unwrap_or(false)
}

fn inquiry(args: &[String]) -> bool {
    let asked: Vec<&String> = args
        .iter()
        .filter(|arg| {
            !arg.trim_start_matches(|c: char| c.is_ascii_digit())
                .starts_with(['>', '<'])
        })
        .collect();
    // complexity: bounded by one BTreeSet lookup per argument
    !asked.is_empty() && asked.iter().all(|arg| rules().inquiry_flags.contains(*arg))
}

fn risky(step: &Step, workspace: &Path, gate: Gate) -> Option<&'static str> {
    let verb = step.verb()?;
    let args = &step.argv()[1..];
    let cwd = step.cwd.as_deref();
    let lower = step.text.to_ascii_lowercase();
    if let Some(hazard) = rules()
        .text_hazards
        .iter()
        // complexity: bounded by text_hazards in rules.toml
        .find(|hazard| lower.contains(&hazard.text))
    {
        return Some(hazard.reason.as_str());
    }
    match &verb_row(&verb)?.hazard {
        Hazard::None => None,
        Hazard::Args(_) if inquiry(args) => None,
        Hazard::Args(rules) => args_reason(rules, args, workspace, cwd, gate),
        Hazard::Sub { skip, rules } => sub_reason(skip, rules, args, gate),
    }
}

fn args_reason(
    rules: &'static [Rule],
    args: &[String],
    workspace: &Path,
    cwd: Option<&Path>,
    gate: Gate,
) -> Option<&'static str> {
    rules
        .iter()
        .find(|rule| {
            rule.gate <= gate
                && any_hit(&rule.when, args)
                && !(rule.broad && narrow(args, workspace, cwd))
        })
        .map(|rule| rule.reason.as_str())
}

fn sub_reason(
    skip: &std::collections::BTreeSet<String>,
    rules: &'static [SubRule],
    args: &[String],
    gate: Gate,
) -> Option<&'static str> {
    let (sub, rest) = subcommand(args, skip)?;
    if inquiry(rest) {
        return None;
    }
    rules
        .iter()
        // complexity: bounded by one BTreeSet lookup per subcommand rule
        .find(|rule| rule.gate <= gate && rule.names.contains(sub) && any_hit(&rule.when, rest))
        .map(|rule| rule.reason.as_str())
}

fn narrow(args: &[String], workspace: &Path, cwd: Option<&Path>) -> bool {
    let targets = found_roots(args);
    let home = summonr_core::dirs::home_dir().map(|home| super::paths::resolved(&home));
    let workspace = super::paths::resolved(workspace);
    let workspace = workspace.as_path();
    !targets.is_empty()
        && targets.iter().all(|target| {
            cwd.and_then(|cwd| resolve(target, cwd))
                .map(|path| super::paths::resolved(&path))
                .is_some_and(|path| !broad(&path, workspace, home.as_deref()))
        })
}

fn found_roots(args: &[String]) -> Vec<&String> {
    let mut targets = Vec::new();
    let mut words = args.iter();
    while let Some(word) = words.next() {
        if word == "$(find" {
            let inner: Vec<&String> = words
                .by_ref()
                .take_while(|word| !word.ends_with(')'))
                .collect();
            let roots: Vec<&String> = inner
                .into_iter()
                .take_while(|word| !word.starts_with('-'))
                .collect();
            if roots.is_empty() {
                targets.push(word);
            }
            targets.extend(roots);
        } else if !word.starts_with('-') {
            targets.push(word);
        }
    }
    targets
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
    let roots = &rules().roots;
    let shallow = match parts.as_slice() {
        [] => true,
        [first] => roots.top.contains(first) || !path.is_absolute(),
        [first, _] => roots.nested.contains(first),
        [_, _, _, ..] => false,
    };
    let guarded = |root: &Path| root.starts_with(path);
    shallow || guarded(workspace) || home.is_some_and(guarded)
}
