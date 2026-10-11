use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use super::{Cwd, is_outside, split_segments, unquoted, verb_row};

pub(crate) struct Step {
    pub(crate) text: String,
    pub(crate) words: Vec<String>,
    pub(crate) cwd: Option<PathBuf>,
    pub(crate) outside: bool,
}

impl Step {
    pub(crate) fn argv(&self) -> &[String] {
        let start = self
            .words
            .iter()
            .position(|word| !is_assignment(word))
            .unwrap_or(self.words.len());
        &self.words[start..]
    }

    pub(crate) fn verb(&self) -> Option<String> {
        verb(self.argv())
    }

    pub(crate) fn redirects(&self) -> Vec<&str> {
        let mut targets = Vec::new();
        let mut words = self.words.iter();
        while let Some(word) = words.next() {
            match redirect(word) {
                Some(Redirect::Next) => {
                    targets.extend(
                        words
                            .next()
                            .map(String::as_str)
                            .filter(|next| !next.starts_with('&')),
                    );
                }
                Some(Redirect::File(target)) => targets.push(target),
                Some(Redirect::Dup) | None => {}
            }
        }
        targets
    }

    pub(crate) fn operands(&self) -> Vec<&str> {
        let mut operands = Vec::new();
        let mut words = self.argv().iter().skip(1);
        while let Some(word) = words.next() {
            match redirect(word) {
                Some(Redirect::Next) => {
                    words.next();
                }
                Some(_) => {}
                None if !word.starts_with('-') => operands.push(word.as_str()),
                None => {}
            }
        }
        operands
    }
}

enum Redirect<'a> {
    Next,
    File(&'a str),
    Dup,
}

fn redirect(word: &str) -> Option<Redirect<'_>> {
    let (_, rest) = word.split_once('>')?;
    let rest = rest.trim_start_matches(['>', '|']);
    if rest.is_empty() {
        return Some(Redirect::Next);
    }
    match rest.strip_prefix('&') {
        Some("") => Some(Redirect::Next),
        Some(fd) if fd == "-" || fd.bytes().all(|b| b.is_ascii_digit()) => Some(Redirect::Dup),
        Some(target) => Some(Redirect::File(target)),
        None => Some(Redirect::File(rest)),
    }
}

pub(crate) fn walk(command: &str, workspace: &Path, start: &Path, roots: &[PathBuf]) -> Vec<Step> {
    let repo = workspace.join(".git").exists();
    let mut vars = HashMap::new();
    let mut cwd = Some(start.to_path_buf());
    let mut outside =
        !start.starts_with(workspace) && !roots.iter().any(|root| start.starts_with(root));
    let mut steps = Vec::new();
    for segment in split_segments(without_heredocs(command.trim()).trim()) {
        let segment = if repo && !outside {
            segment.replace("$(git rev-parse --show-toplevel)", ".")
        } else {
            segment
        };
        let text = expand_vars(&segment, &vars);
        let words: Vec<String> = shell_words(&text)
            .into_iter()
            .map(|word| word.replace(['\'', '"'], ""))
            .collect();
        assign(&words, &mut vars);
        let step = Step {
            text,
            words,
            cwd: cwd.clone(),
            outside,
        };
        if let Some(target) = changed_dir(&step) {
            match target {
                Some(target) => {
                    cwd = cwd.as_deref().and_then(|cwd| resolve(target, cwd));
                    outside |= is_outside(target, workspace, outside, roots);
                }
                None => {
                    cwd = None;
                    outside = true;
                }
            }
        }
        steps.push(step);
    }
    steps
}

fn changed_dir(step: &Step) -> Option<Option<&str>> {
    let row = verb_row(&step.verb()?)?;
    match row.cwd {
        Cwd::Tracked => Some(
            step.argv()
                .iter()
                .skip(1)
                .find(|word| !word.starts_with('-'))
                .map(String::as_str),
        ),
        Cwd::Changes => Some(None),
        Cwd::Stays => None,
    }
}

pub(crate) fn shell_words(segment: &str) -> Vec<String> {
    unquoted(segment, &[" ", "\t"])
        .into_iter()
        .map(|word| word.trim_matches(|c| c == '"' || c == '\'').to_string())
        .filter(|word| !word.is_empty())
        .collect()
}

pub(crate) fn argv(segment: &str) -> Vec<String> {
    let words = shell_words(segment);
    let start = words
        .iter()
        .position(|word| !is_assignment(word))
        .unwrap_or(words.len());
    words[start..].to_vec()
}

pub(crate) fn verb(words: &[String]) -> Option<String> {
    let name = words.first()?.rsplit('/').next()?.replace('\\', "");
    let name = name.to_ascii_lowercase();
    Some(name.strip_suffix(".exe").unwrap_or(&name).to_string())
}

fn is_assignment(word: &str) -> bool {
    word.contains('=') && !word.starts_with('-')
}

fn assign(words: &[String], vars: &mut HashMap<String, String>) {
    let pairs: Option<Vec<(&str, &str)>> = words
        .iter()
        .map(|word| {
            let (name, value) = word.split_once('=')?;
            let named = !name.is_empty() && name.chars().all(is_name_char);
            named.then_some((name, value))
        })
        .collect();
    for (name, value) in pairs.unwrap_or_default() {
        if value.contains(['$', '`']) {
            vars.remove(name);
        } else {
            vars.insert(name.to_string(), value.to_string());
        }
    }
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn expand_vars(segment: &str, vars: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(segment.len());
    let mut rest = segment;
    while let Some(at) = rest.find('$') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let (name, used) = match after.strip_prefix('{') {
            Some(inner) => match inner.find('}') {
                Some(close) => (&inner[..close], close + 2),
                None => ("", 0),
            },
            None => {
                let len = after
                    .find(|c: char| !is_name_char(c))
                    .unwrap_or(after.len());
                (&after[..len], len)
            }
        };
        match vars.get(name) {
            Some(value) => {
                out.push_str(value);
                rest = &after[used..];
            }
            None => {
                out.push('$');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

pub(crate) fn resolve(target: &str, cwd: &Path) -> Option<PathBuf> {
    if target.contains(['$', '`']) || target.starts_with('~') {
        return None;
    }
    let base = if Path::new(target).is_absolute() {
        PathBuf::new()
    } else {
        cwd.to_path_buf()
    };
    Some(normalize_onto(base, Path::new(target)))
}

pub(crate) fn normalize(path: &Path) -> PathBuf {
    normalize_onto(PathBuf::new(), path)
}

fn normalize_onto(base: PathBuf, path: &Path) -> PathBuf {
    path.components().fold(base, |mut path, part| {
        match part {
            Component::ParentDir => {
                path.pop();
            }
            Component::CurDir => {}
            other => path.push(other),
        }
        path
    })
}

pub(crate) fn without_heredocs(command: &str) -> String {
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

pub(crate) fn heredocs(command: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut open: Option<(String, String, Vec<&str>)> = None;
    for line in command.lines() {
        if let Some((end, opening, body)) = &mut open {
            if line.trim() == end.as_str() {
                found.push((std::mem::take(opening), body.join("\n")));
                open = None;
            } else {
                body.push(line);
            }
            continue;
        }
        if let Some(end) = heredoc_end(line) {
            open = Some((end, line.to_string(), Vec::new()));
        }
    }
    found
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
