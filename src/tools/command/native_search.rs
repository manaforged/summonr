use serde_json::{Map, Value, json};

use crate::policy::{Clip, Native, verb_row};

const QUIET: &[&str] = &["2>/dev/null", "2>&1"];
const SHELL: &[char] = &['&', ';', '>', '<', '`', '$', '(', ')', '\n'];

#[derive(Clone, Copy)]
enum Grep {
    Fold,
    Files,
    Count,
    Word,
    Literal,
    NoIgnore,
    Skip,
    Glob,
    Pattern,
    Context,
}

impl Grep {
    const fn takes_value(self) -> bool {
        matches!(self, Self::Glob | Self::Pattern | Self::Context)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Find {
    Name,
    NameFolded,
    Depth,
    FilesOnly,
    Words,
    Within,
    Not,
    Path,
}

const GREP: &[(&str, Grep)] = &[
    ("-i", Grep::Fold),
    ("--ignore-case", Grep::Fold),
    ("-l", Grep::Files),
    ("--files-with-matches", Grep::Files),
    ("-c", Grep::Count),
    ("--count", Grep::Count),
    ("-w", Grep::Word),
    ("--word-regexp", Grep::Word),
    ("-F", Grep::Literal),
    ("--fixed-strings", Grep::Literal),
    ("-u", Grep::NoIgnore),
    ("--no-ignore", Grep::NoIgnore),
    ("-e", Grep::Pattern),
    ("-g", Grep::Glob),
    ("--glob", Grep::Glob),
    ("--include", Grep::Glob),
    ("-C", Grep::Context),
    ("--context", Grep::Context),
    ("-r", Grep::Skip),
    ("-R", Grep::Skip),
    ("-n", Grep::Skip),
    ("-H", Grep::Skip),
    ("-I", Grep::Skip),
    ("-s", Grep::Skip),
    ("-E", Grep::Skip),
    ("-S", Grep::Skip),
    ("--recursive", Grep::Skip),
    ("--line-number", Grep::Skip),
    ("--hidden", Grep::Skip),
    ("--no-messages", Grep::Skip),
];

const FIND: &[(&str, Find)] = &[
    ("-name", Find::Name),
    ("-iname", Find::NameFolded),
    ("-maxdepth", Find::Depth),
    ("-type", Find::FilesOnly),
    ("-not", Find::Not),
    ("!", Find::Not),
    ("-path", Find::Path),
];

const MDFIND: &[(&str, Find)] = &[("-name", Find::Words), ("-onlyin", Find::Within)];

pub(crate) fn native_search_args(command: &str) -> Option<Value> {
    let (head, limit) = match command.split_once('|') {
        Some((head, stage)) => (head, Some(head_count(stage)?)),
        None => (command, None),
    };
    // complexity: bounded by the two fixed quiet redirects
    let words: Vec<String> = crate::policy::argv(head)
        .into_iter()
        .filter(|word| !QUIET.contains(&word.as_str()))
        .collect();
    // complexity: bounded by the nine shell characters
    if words.iter().any(|word| word.contains(SHELL)) {
        return None;
    }
    let verb = words.first()?.rsplit('/').next()?;
    let mut args = match verb_row(verb)?.native? {
        Native::Grep => content(&words[1..], true)?,
        Native::Rg => content(&words[1..], false)?,
        Native::Find => names(&words[1..], FIND)?,
        Native::Mdfind => names(&words[1..], MDFIND)?,
        Native::Cat | Native::Head | Native::Sed => return None,
    };
    if let Some(limit) = limit {
        set(&mut args, "limit", json!(limit));
    }
    Some(Value::Object(args))
}

fn head_count(stage: &str) -> Option<usize> {
    let mut words = stage.split_whitespace();
    let verb = words.next()?.rsplit('/').next()?;
    matches!(verb_row(verb)?.clip?, Clip::Head).then_some(())?;
    let (first, second) = (words.next(), words.next());
    if words.next().is_some() {
        return None;
    }
    let count = match (first, second) {
        (None, None) => "10".to_string(),
        (Some(flag), None) => flag
            .strip_prefix("-n")
            .or_else(|| flag.strip_prefix('-'))?
            .to_string(),
        (Some(flag), Some(count)) => {
            (flag == "-n").then_some(())?;
            count.to_string()
        }
        (None, Some(_)) => return None,
    };
    count.parse().ok()
}

fn set(args: &mut Map<String, Value>, key: &str, value: Value) {
    args.insert(key.to_string(), value);
}

fn lookup<F: Copy>(table: &[(&str, F)], word: &str) -> Option<F> {
    table
        .iter()
        .find(|(name, _)| *name == word)
        .map(|(_, flag)| *flag)
}

fn flags_of(word: &str) -> Option<Vec<Grep>> {
    if let Some(flag) = lookup(GREP, word) {
        return Some(vec![flag]);
    }
    if let Some((name, value)) = word.split_once('=') {
        return lookup(GREP, name)
            .filter(|flag| flag.takes_value() && !value.is_empty())
            .map(|flag| vec![flag]);
    }
    word.strip_prefix('-')
        .filter(|short| !short.starts_with('-'))?
        .chars()
        .map(|letter| lookup(GREP, &format!("-{letter}")).filter(|flag| !flag.takes_value()))
        .collect()
}

fn content(words: &[String], plain_grep: bool) -> Option<Map<String, Value>> {
    let mut args = Map::new();
    let mut explicit: Option<String> = None;
    let mut operands: Vec<&str> = Vec::new();
    let mut fold = false;
    let mut at = 0;
    while let Some(word) = words.get(at) {
        at += 1;
        if !word.starts_with('-') || word == "-" {
            operands.push(word);
            continue;
        }
        let inline = word.split_once('=').map(|(_, value)| value.to_string());
        for flag in flags_of(word)? {
            let value = match (flag.takes_value(), &inline) {
                (false, _) => String::new(),
                (true, Some(value)) => value.clone(),
                (true, None) => {
                    at += 1;
                    words.get(at - 1)?.clone()
                }
            };
            match flag {
                Grep::Fold => fold = true,
                Grep::Files => set(&mut args, "output", json!("files")),
                Grep::Count => set(&mut args, "output", json!("count")),
                Grep::Word => set(&mut args, "word", json!(true)),
                Grep::Literal => set(&mut args, "literal", json!(true)),
                Grep::NoIgnore => set(&mut args, "no_ignore", json!(true)),
                Grep::Pattern => explicit = Some(value),
                Grep::Glob => set(&mut args, "glob", json!(value)),
                Grep::Context => set(&mut args, "context", json!(value.parse::<usize>().ok()?)),
                Grep::Skip => {}
            }
        }
    }
    let (pattern, paths): (&str, &[&str]) = match &explicit {
        Some(pattern) => (pattern, operands.as_slice()),
        None => operands.split_first().map(|(first, rest)| (*first, rest))?,
    };
    set(&mut args, "pattern", json!(pattern));
    if !fold {
        set(&mut args, "case_sensitive", json!(true));
    }
    if plain_grep {
        set(&mut args, "no_ignore", json!(true));
    }
    place(&mut args, paths);
    Some(args)
}

fn names(words: &[String], table: &[(&str, Find)]) -> Option<Map<String, Value>> {
    let mut args = Map::new();
    let mut paths: Vec<&str> = Vec::new();
    let mut at = 0;
    while let Some(word) = words.get(at) {
        at += 1;
        let Some(flag) = lookup(table, word) else {
            (!word.starts_with('-') && args.is_empty()).then_some(())?;
            paths.push(word);
            continue;
        };
        if flag == Find::Not {
            (lookup(table, words.get(at)?)? == Find::Path).then_some(())?;
            at += 1;
        }
        let value = words.get(at)?;
        at += 1;
        if flag == Find::Within {
            paths.push(value);
        } else {
            find_flag(&mut args, flag, value)?;
        }
    }
    (args.contains_key("glob") || args.contains_key("name")).then_some(())?;
    set(&mut args, "no_ignore", json!(true));
    if paths.is_empty() && args.contains_key("name") {
        paths.push("/");
    }
    place(&mut args, &paths);
    Some(args)
}

fn find_flag(args: &mut Map<String, Value>, flag: Find, value: &str) -> Option<()> {
    match flag {
        Find::Name => {
            set(args, "glob", json!(value));
            set(args, "case_sensitive", json!(true));
        }
        Find::NameFolded => set(args, "glob", json!(value)),
        Find::Depth => set(args, "depth", json!(value.parse::<usize>().ok()?)),
        Find::FilesOnly => (value == "f").then_some(())?,
        Find::Words => set(args, "name", json!(value)),
        Find::Not => set(args, "exclude", json!(anywhere(value))),
        Find::Path | Find::Within => return None,
    }
    Some(())
}

fn anywhere(path_glob: &str) -> String {
    if !path_glob.contains('/') {
        return path_glob.to_string();
    }
    let inner = path_glob.strip_prefix("*/").unwrap_or(path_glob);
    let inner = inner.strip_suffix("/*").unwrap_or(inner);
    format!("**/{inner}/**")
}

fn place(args: &mut Map<String, Value>, paths: &[&str]) {
    match paths {
        [] => {}
        [path] => set(args, "path", json!(path)),
        many => set(args, "path", json!(many)),
    }
}
