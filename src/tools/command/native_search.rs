use serde_json::{Map, Value, json};

use crate::policy::Native;

const QUIET: &[&str] = &["2>/dev/null", "2>&1"];
const SHELL: &[char] = &['&', ';', '>', '<', '`', '$', '(', ')', '\n'];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Flag {
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
    Name,
    NameFolded,
    Depth,
    FilesOnly,
    Words,
    Within,
    Not,
    Exclude,
}

impl Flag {
    const fn takes_value(self) -> bool {
        matches!(
            self,
            Self::Glob
                | Self::Pattern
                | Self::Context
                | Self::Name
                | Self::NameFolded
                | Self::Depth
                | Self::FilesOnly
                | Self::Words
                | Self::Within
                | Self::Exclude
        )
    }
}

const GREP: &[(&str, Flag)] = &[
    ("-i", Flag::Fold),
    ("--ignore-case", Flag::Fold),
    ("-l", Flag::Files),
    ("--files-with-matches", Flag::Files),
    ("-c", Flag::Count),
    ("--count", Flag::Count),
    ("-w", Flag::Word),
    ("--word-regexp", Flag::Word),
    ("-F", Flag::Literal),
    ("--fixed-strings", Flag::Literal),
    ("-u", Flag::NoIgnore),
    ("--no-ignore", Flag::NoIgnore),
    ("-e", Flag::Pattern),
    ("-g", Flag::Glob),
    ("--glob", Flag::Glob),
    ("--include", Flag::Glob),
    ("-C", Flag::Context),
    ("--context", Flag::Context),
    ("-r", Flag::Skip),
    ("-R", Flag::Skip),
    ("-n", Flag::Skip),
    ("-H", Flag::Skip),
    ("-I", Flag::Skip),
    ("-s", Flag::Skip),
    ("-E", Flag::Skip),
    ("-S", Flag::Skip),
    ("--recursive", Flag::Skip),
    ("--line-number", Flag::Skip),
    ("--hidden", Flag::Skip),
    ("--no-messages", Flag::Skip),
];

const FIND: &[(&str, Flag)] = &[
    ("-name", Flag::Name),
    ("-iname", Flag::NameFolded),
    ("-maxdepth", Flag::Depth),
    ("-type", Flag::FilesOnly),
    ("-not", Flag::Not),
    ("!", Flag::Not),
    ("-path", Flag::Exclude),
];

const MDFIND: &[(&str, Flag)] = &[("-name", Flag::Words), ("-onlyin", Flag::Within)];

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
    let mut args = match crate::policy::verb_row(verb)?.native? {
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
    matches!(
        crate::policy::verb_row(verb)?.clip?,
        crate::policy::Clip::Head
    )
    .then_some(())?;
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

fn lookup(table: &[(&str, Flag)], word: &str) -> Option<Flag> {
    table
        .iter()
        .find(|(name, _)| *name == word)
        .map(|(_, flag)| *flag)
}

fn flags_of(word: &str) -> Option<Vec<Flag>> {
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
                Flag::Fold => fold = true,
                Flag::Files => set(&mut args, "output", json!("files")),
                Flag::Count => set(&mut args, "output", json!("count")),
                Flag::Word => set(&mut args, "word", json!(true)),
                Flag::Literal => set(&mut args, "literal", json!(true)),
                Flag::NoIgnore => set(&mut args, "no_ignore", json!(true)),
                Flag::Pattern => explicit = Some(value),
                Flag::Glob => set(&mut args, "glob", json!(value)),
                Flag::Context => set(&mut args, "context", json!(value.parse::<usize>().ok()?)),
                Flag::Skip => {}
                Flag::Name
                | Flag::NameFolded
                | Flag::Depth
                | Flag::FilesOnly
                | Flag::Words
                | Flag::Within
                | Flag::Not
                | Flag::Exclude => return None,
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

fn names(words: &[String], table: &[(&str, Flag)]) -> Option<Map<String, Value>> {
    let mut args = Map::new();
    let mut paths: Vec<&str> = Vec::new();
    let mut at = 0;
    while let Some(word) = words.get(at) {
        at += 1;
        let Some(mut flag) = lookup(table, word) else {
            (!word.starts_with('-') && args.is_empty()).then_some(())?;
            paths.push(word);
            continue;
        };
        if flag == Flag::Not {
            (lookup(table, words.get(at)?)? == Flag::Exclude).then_some(())?;
            at += 1;
            flag = Flag::Exclude;
        } else if flag == Flag::Exclude {
            return None;
        }
        let value = words.get(at)?;
        at += 1;
        match flag {
            Flag::Name => {
                set(&mut args, "glob", json!(value));
                set(&mut args, "case_sensitive", json!(true));
            }
            Flag::NameFolded => set(&mut args, "glob", json!(value)),
            Flag::Depth => set(&mut args, "depth", json!(value.parse::<usize>().ok()?)),
            Flag::FilesOnly => (value == "f").then_some(())?,
            Flag::Words => set(&mut args, "name", json!(value)),
            Flag::Within => paths.push(value),
            Flag::Exclude => set(&mut args, "exclude", json!(anywhere(value))),
            Flag::Not
            | Flag::Fold
            | Flag::Files
            | Flag::Count
            | Flag::Word
            | Flag::Literal
            | Flag::NoIgnore
            | Flag::Skip
            | Flag::Glob
            | Flag::Pattern
            | Flag::Context => return None,
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
