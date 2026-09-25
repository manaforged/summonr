use serde::Serialize;

use crate::policy::{Clip, GREP_LONG, GREP_SHORT, GrepOpt, Native, SearchFlag, SearchValue};

#[derive(Clone, Copy)]
pub(crate) enum Pipe {
    Head(usize),
    Tail(usize),
}

impl Pipe {
    pub(crate) fn apply(self, text: &str) -> String {
        let lines: Vec<&str> = text.lines().collect();
        let (keep, skipped) = match self {
            Pipe::Head(n) => (n.min(lines.len()), lines.len().saturating_sub(n)),
            Pipe::Tail(n) => (n.min(lines.len()), lines.len().saturating_sub(n)),
        };
        if skipped == 0 {
            return text.trim_end().to_string();
        }
        let mut kept: Vec<String> = match self {
            Pipe::Head(_) => lines[..keep].iter().map(|l| l.to_string()).collect(),
            Pipe::Tail(_) => lines[lines.len() - keep..]
                .iter()
                .map(|l| l.to_string())
                .collect(),
        };
        kept.push(format!("[{skipped} lines clipped]"));
        kept.join("\n")
    }
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Op {
    List,
    Search,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReadArgs {
    #[serde(skip_serializing_if = "Option::is_none")]
    op: Option<Op>,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pattern: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    offset: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    limit: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    recursive: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    files: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    count: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    case_sensitive: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    literal: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    after: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    before: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<usize>,
}

impl ReadArgs {
    fn op(op: Op) -> Self {
        Self {
            op: Some(op),
            ..Self::default()
        }
    }

    fn path(path: String) -> Self {
        Self {
            path: Some(path),
            ..Self::default()
        }
    }

    pub(crate) fn to_json(&self) -> String {
        serde_json::json!(self).to_string()
    }

    fn set(&mut self, flag: SearchFlag) {
        match flag {
            SearchFlag::Files => self.files = Some(true),
            SearchFlag::Count => self.count = Some(true),
            SearchFlag::IgnoreCase => self.case_sensitive = Some(false),
            SearchFlag::Literal => self.literal = Some(true),
        }
    }
}

pub(crate) fn native_read_args(command: &str) -> Option<(ReadArgs, Option<Pipe>)> {
    let (command, pipe) = match command.split_once('|') {
        Some((head, stage)) => (head.trim_end(), Some(pipe_stage(stage.trim())?)),
        None => (command, None),
    };
    if unsupported(command) {
        return None;
    }
    let words = crate::policy::argv(command);
    let verb = words.first()?.rsplit('/').next()?;
    let args = match crate::policy::verb_row(verb)?.native? {
        Native::Cat if words.len() == 2 => ReadArgs::path(positional(&words).next()?.clone()),
        Native::Cat => return None,
        Native::Head => head(&words)?,
        Native::Ls => list(
            &words,
            words.iter().any(|w| w.starts_with('-') && w.contains('R')),
        ),
        Native::Tree => list(&words, true),
        Native::Wc if words.iter().any(|w| w == "-l") => ReadArgs {
            limit: Some(1),
            ..ReadArgs::path(positional(&words).next()?.clone())
        },
        Native::Wc => return None,
        Native::Sed => sed(&words)?,
        Native::Search { basic } => search(&words, basic)?,
    };
    Some((args, pipe))
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

fn pipe_stage(command: &str) -> Option<Pipe> {
    let mut words = command.split_whitespace();
    let verb = words.next()?.rsplit('/').next()?;
    let n = match (words.next(), words.next(), words.next()) {
        (None, None, None) => 10,
        (Some(flag), None, None) => flag
            .strip_prefix("-n")
            .and_then(|n| n.parse().ok())
            .or_else(|| flag.strip_prefix('-')?.parse().ok())?,
        (Some("-n"), Some(n), None) => n.parse().ok()?,
        _other => return None,
    };
    match crate::policy::verb_row(verb)?.clip? {
        Clip::Head => Some(Pipe::Head(n)),
        Clip::Tail => Some(Pipe::Tail(n)),
    }
}

fn positional(words: &[String]) -> impl DoubleEndedIterator<Item = &String> {
    words[1..].iter().filter(|word| !word.starts_with('-'))
}

fn head(words: &[String]) -> Option<ReadArgs> {
    let path = positional(words).next_back()?.clone();
    let limit = flag_value(words, "-n")
        .or_else(|| {
            words[1..]
                .iter()
                .find_map(|w| w.strip_prefix('-')?.parse::<usize>().ok())
        })
        .unwrap_or(10);
    Some(ReadArgs {
        limit: Some(limit),
        ..ReadArgs::path(path)
    })
}

fn list(words: &[String], recursive: bool) -> ReadArgs {
    ReadArgs {
        path: positional(words).next().cloned(),
        recursive: recursive.then_some(true),
        ..ReadArgs::op(Op::List)
    }
}

fn sed(words: &[String]) -> Option<ReadArgs> {
    let script = words
        .iter()
        .position(|w| w == "-e")
        .and_then(|i| words.get(i + 1))
        .or_else(|| {
            words
                .iter()
                .find(|w| w.contains('p') && w.contains(|c: char| c.is_ascii_digit()))
        })?;
    let file = positional(words).next_back()?.clone();
    let range = script.trim_matches(['\'', '"']).trim_end_matches('p');
    let (start, end): (usize, usize) = match range.split_once(',') {
        Some((a, b)) => (a.parse().ok()?, b.parse().ok()?),
        None => {
            let n = range.parse().ok()?;
            (n, n)
        }
    };
    if !words.iter().any(|w| w == "-n") || start == 0 || end < start {
        return None;
    }
    Some(ReadArgs {
        offset: Some(start),
        limit: Some(end - start + 1),
        ..ReadArgs::path(file)
    })
}

struct Search {
    args: ReadArgs,
    pattern: Option<String>,
    extended: bool,
}

impl Search {
    fn value(&mut self, value: SearchValue, text: Option<&String>) -> Option<()> {
        let number = || text?.parse().ok();
        match value {
            SearchValue::Pattern => self.pattern = text.cloned(),
            SearchValue::After => self.args.after = Some(number()?),
            SearchValue::Before => self.args.before = Some(number()?),
            SearchValue::Context => self.args.context = Some(number()?),
        }
        Some(())
    }

    fn long_flag<'a>(
        &mut self,
        flag: &str,
        rest: &mut impl Iterator<Item = &'a String>,
    ) -> Option<()> {
        let (_, opt) = GREP_LONG.iter().find(|(name, _)| *name == flag)?;
        match *opt {
            GrepOpt::Set(flag) => self.args.set(flag),
            GrepOpt::Value(value) => self.value(value, rest.next())?,
            GrepOpt::Extended => self.extended = true,
            GrepOpt::Skip => {}
            GrepOpt::Reject => return None,
        }
        Some(())
    }

    fn short_flags<'a>(
        &mut self,
        flags: &str,
        rest: &mut impl Iterator<Item = &'a String>,
    ) -> Option<()> {
        for (at, flag) in flags.char_indices() {
            let (_, opt) = GREP_SHORT.iter().find(|(name, _)| *name == flag)?;
            match *opt {
                GrepOpt::Set(flag) => self.args.set(flag),
                GrepOpt::Value(value) => {
                    let tail = &flags[at + flag.len_utf8()..];
                    let owned = (!tail.is_empty()).then(|| tail.to_string());
                    let text = owned.as_ref().or_else(|| rest.next());
                    return self.value(value, text);
                }
                GrepOpt::Extended => self.extended = true,
                GrepOpt::Skip => {}
                GrepOpt::Reject => return None,
            }
        }
        Some(())
    }
}

fn search(words: &[String], basic: bool) -> Option<ReadArgs> {
    let mut state = Search {
        args: ReadArgs::op(Op::Search),
        pattern: None,
        extended: false,
    };
    let mut rest = words[1..].iter();
    while let Some(word) = rest.next() {
        match word.strip_prefix('-') {
            Some(flag) => match flag.strip_prefix('-') {
                Some(long) => state.long_flag(long, &mut rest)?,
                None => state.short_flags(flag, &mut rest)?,
            },
            None if state.pattern.is_none() => state.pattern = Some(word.clone()),
            None if state.args.path.is_none() => state.args.path = Some(word.clone()),
            None => return None,
        }
    }
    let basic = basic && !state.extended && state.args.literal.is_none();
    let pattern = state
        .pattern
        .filter(|p| !basic || !p.contains(['{', '+']))?;
    Some(ReadArgs {
        pattern: Some(pattern),
        ..state.args
    })
}

fn flag_value(words: &[String], flag: &str) -> Option<usize> {
    words
        .iter()
        .position(|w| w == flag)
        .and_then(|i| words.get(i + 1))
        .and_then(|w| w.parse().ok())
}
