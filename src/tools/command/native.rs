use serde::Serialize;

use crate::policy::{Clip, Native};

#[derive(Clone, Copy)]
pub(crate) enum Pipe {
    Head(usize),
    Tail(usize),
}

impl Pipe {
    pub(crate) fn apply(self, text: &str) -> String {
        let lines: Vec<&str> = text.lines().collect();
        let (Pipe::Head(keep) | Pipe::Tail(keep)) = self;
        let skipped = lines.len().saturating_sub(keep);
        if skipped == 0 {
            return text.trim_end().to_string();
        }
        let mut kept = match self {
            Pipe::Head(_) => lines[..keep].to_vec(),
            Pipe::Tail(_) => lines[skipped..].to_vec(),
        };
        let mark = format!("[{skipped} lines clipped]");
        kept.push(&mark);
        kept.join("\n")
    }
}

#[derive(Serialize)]
pub(crate) struct ReadArgs {
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    offset: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    limit: Option<usize>,
}

impl ReadArgs {
    fn path(path: String) -> Self {
        Self {
            path,
            offset: None,
            limit: None,
        }
    }

    pub(crate) fn value(&self) -> serde_json::Value {
        serde_json::json!(self)
    }
}

pub(crate) fn native_read_args(command: &str) -> Option<(ReadArgs, Option<Pipe>)> {
    let (command, pipe) = split_pipe(command)?;
    if unsupported(command) {
        return None;
    }
    let args = read_args(&crate::policy::argv(command), pipe)?;
    Some((args, pipe))
}

fn split_pipe(command: &str) -> Option<(&str, Option<Pipe>)> {
    match command.split_once('|') {
        Some((head, stage)) => Some((head.trim_end(), Some(pipe_stage(stage.trim())?))),
        None => Some((command, None)),
    }
}

fn read_args(words: &[String], pipe: Option<Pipe>) -> Option<ReadArgs> {
    let verb = words.first()?.rsplit('/').next()?;
    match crate::policy::verb_row(verb)?.native? {
        Native::Cat => cat(words, pipe),
        Native::Head => head(words),
        Native::Sed => sed(words),
        Native::Grep | Native::Rg | Native::Find | Native::Mdfind => None,
    }
}

fn unsupported(command: &str) -> bool {
    command.chars().any(|c| {
        matches!(
            c,
            '|' | '&' | ';' | '>' | '<' | '`' | '$' | '(' | ')' | '*' | '?' | '[' | '~' | '\n'
        )
    }) || command.contains(" -c")
        || command.contains(" --bytes")
}

pub(super) fn pipe_stage(command: &str) -> Option<Pipe> {
    let mut words = command.split_whitespace();
    let verb = words.next()?.rsplit('/').next()?;
    let n = match (words.next(), words.next(), words.next()) {
        (None, None, None) => 10,
        (Some(flag), None, None) => count_flag(flag)?,
        (Some("-n"), Some(n), None) => n.parse().ok()?,
        _other => return None,
    };
    match crate::policy::verb_row(verb)?.clip? {
        Clip::Head => Some(Pipe::Head(n)),
        Clip::Tail => Some(Pipe::Tail(n)),
    }
}

fn count_flag(flag: &str) -> Option<usize> {
    flag.strip_prefix("-n")
        .and_then(|n| n.parse().ok())
        .or_else(|| flag.strip_prefix('-')?.parse().ok())
}

fn cat(words: &[String], pipe: Option<Pipe>) -> Option<ReadArgs> {
    let [_, path] = words else {
        return None;
    };
    if path.starts_with('-') || matches!(pipe, Some(Pipe::Tail(_))) {
        return None;
    }
    Some(ReadArgs::path(path.clone()))
}

fn head(words: &[String]) -> Option<ReadArgs> {
    let args: Vec<&str> = words.iter().map(String::as_str).collect();
    let (limit, file) = match args.as_slice() {
        [_, "-n", count, file] => (count.parse().ok()?, *file),
        [_, flag, file] => (count_flag(flag)?, *file),
        [_, file] => (10, *file),
        [] | [_] | [_, _, _, _, ..] => return None,
    };
    if file.starts_with('-') {
        return None;
    }
    Some(ReadArgs {
        limit: Some(limit),
        ..ReadArgs::path(file.to_string())
    })
}

fn sed(words: &[String]) -> Option<ReadArgs> {
    let args: Vec<&str> = words.iter().map(String::as_str).collect();
    let [_, "-n", script @ .., file] = args.as_slice() else {
        return None;
    };
    let (["-e", script] | [script]) = script else {
        return None;
    };
    if file.starts_with('-') {
        return None;
    }
    let (start, end) = line_range(script)?;
    Some(ReadArgs {
        offset: Some(start),
        limit: Some(end - start + 1),
        ..ReadArgs::path((*file).to_string())
    })
}

fn line_range(script: &str) -> Option<(usize, usize)> {
    let range = script.strip_suffix('p')?;
    let (start, end): (usize, usize) = match range.split_once(',') {
        Some((first, last)) => (first.parse().ok()?, last.parse().ok()?),
        None => {
            let line = range.parse().ok()?;
            (line, line)
        }
    };
    (start > 0 && end >= start).then_some((start, end))
}
