use crate::policy::Grant;

use super::ToolRegistry;

pub(super) fn plan_write_note(read_only: bool, raw: &str) -> String {
    if read_only && raw.contains("Operation not permitted") {
        "\nplan mode runs the shell read-only. Return the plan in your reply. Switch to auto mode before building or editing files."
            .to_string()
    } else {
        String::new()
    }
}

pub(super) fn sandbox_grant(registry: &ToolRegistry, command: &str, raw: &str) -> Option<Grant> {
    let eperm = raw.contains("Operation not permitted") || raw.contains("Permission denied");
    let sandbox_denied = raw.contains("Operation not permitted");
    if eperm
        && let Some(grant) = crate::policy::outside_write(
            command,
            &registry.workspace,
            &registry.policy.write_roots(),
        )
    {
        return Some(grant);
    }
    if eperm && let Some(path) = crate::policy::implicit_write(command) {
        return Some(Grant::Write(path));
    }
    if registry.denies_network()
        && (network_text(raw) || ((eperm || weak_network_text(raw)) && network_verb(command)))
    {
        return Some(Grant::Network);
    }
    if registry.has_sandbox() && sandbox_denied {
        return Some(Grant::Command(command.to_string()));
    }
    None
}

fn network_text(raw: &str) -> bool {
    const MARKERS: [&str; 5] = [
        "Could not resolve host",
        "Network is unreachable",
        "Temporary failure in name resolution",
        "nodename nor servname",
        "curl: (6)",
    ];
    MARKERS.iter().any(|marker| raw.contains(marker))
}

fn weak_network_text(raw: &str) -> bool {
    const MARKERS: [&str; 3] = [
        "Failed to connect",
        "Could not connect",
        "Connection refused",
    ];
    MARKERS.iter().any(|marker| raw.contains(marker))
}

fn network_verb(command: &str) -> bool {
    segment_verbs(command).any(|verb| {
        matches!(
            verb.strip_suffix(".exe").unwrap_or(&verb),
            "curl" | "wget" | "ssh" | "scp" | "sftp"
        )
    })
}

pub(super) fn segment_verbs(command: &str) -> impl Iterator<Item = String> {
    crate::policy::split_segments(command)
        .into_iter()
        .filter_map(|segment| crate::policy::verb(&crate::policy::argv(&segment)))
}

pub(super) fn sandbox_note(grant: Option<&Grant>, registry: &ToolRegistry, raw: &str) -> String {
    match grant {
        Some(Grant::Network) => {
            "\nnote: network is off in the sandbox. /permissions net on for this session, or /permissions net once, then retry.\n"
                .to_string()
        }
        Some(Grant::Write(path) | Grant::Guarded(path)) => format!(
            "\nnote: {}\n",
            crate::policy::write_denied(&path.display().to_string())
        ),
        Some(Grant::Command(_)) => {
            "\nnote: the OS sandbox denied this command. Approving runs it without the sandbox.\n"
                .to_string()
        }
        Some(Grant::Danger(_) | Grant::Edit(_)) => String::new(),
        None if registry.has_sandbox()
            && (raw.contains("Operation not permitted") || raw.contains("Permission denied")) =>
        {
            if registry.denies_network() {
                "\nnote: the OS sandbox denied this command (network is off, or the write is outside the writable roots).\n"
                    .to_string()
            } else {
                "\nnote: the OS sandbox denied a write outside the writable roots — add the directory to [sandbox] write in ~/.summonr/config.toml.\n"
                    .to_string()
            }
        }
        None => String::new(),
    }
}

pub(super) fn slow_search_note(command: &str, secs: u64) -> String {
    if secs < 30 || !super::digest::searchish(command) {
        return String::new();
    }
    format!(
        "\nnote: this search ran {secs}s; the grep tool skips ignored directories such as target/ and node_modules/"
    )
}

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
    match verb {
        "head" => Some(Pipe::Head(n)),
        "tail" => Some(Pipe::Tail(n)),
        _other => None,
    }
}

pub(crate) fn native_read_args(command: &str) -> Option<(serde_json::Value, Option<Pipe>)> {
    let (command, pipe) = match command.split_once('|') {
        Some((head, stage)) => (head.trim_end(), Some(pipe_stage(stage.trim())?)),
        None => (command, None),
    };
    if command.chars().any(|c| {
        matches!(
            c,
            '|' | '&' | ';' | '>' | '<' | '`' | '$' | '(' | ')' | '*' | '?' | '[' | '~' | '\n'
        )
    }) || command.contains(" -c")
        || command.contains(" --bytes")
    {
        return None;
    }
    let words = crate::policy::argv(command);
    let verb = words.first()?.rsplit('/').next()?;
    let mut positional = words[1..]
        .iter()
        .filter(|word| !word.starts_with('-'))
        .cloned();
    let mut args = serde_json::Map::new();
    match verb {
        "cat" if words.len() == 2 => {
            args.insert("path".into(), positional.next()?.into());
        }
        "head" => {
            let path = positional.next_back()?;
            let limit = flag_value(&words, "-n")
                .or_else(|| {
                    words[1..]
                        .iter()
                        .find_map(|w| w.strip_prefix('-')?.parse::<usize>().ok())
                })
                .unwrap_or(10);
            args.insert("path".into(), path.into());
            args.insert("limit".into(), limit.into());
        }
        "ls" => {
            args.insert("op".into(), "list".into());
            if let Some(path) = positional.next() {
                args.insert("path".into(), path.into());
            }
            if words.iter().any(|w| w.starts_with('-') && w.contains('R')) {
                args.insert("recursive".into(), true.into());
            }
        }
        "tree" => {
            args.insert("op".into(), "list".into());
            args.insert("recursive".into(), true.into());
            if let Some(path) = positional.next() {
                args.insert("path".into(), path.into());
            }
        }
        "wc" if words.iter().any(|w| w == "-l") => {
            args.insert("path".into(), positional.next()?.into());
            args.insert("limit".into(), 1.into());
        }
        "sed" => {
            let script = words
                .iter()
                .position(|w| w == "-e")
                .and_then(|i| words.get(i + 1))
                .cloned()
                .or_else(|| {
                    words
                        .iter()
                        .find(|w| w.contains('p') && w.contains(|c: char| c.is_ascii_digit()))
                        .cloned()
                })?;
            let file = positional.next_back()?;
            let range = script.trim_matches(['\'', '"']).trim_end_matches('p');
            let (start, end) = match range.split_once(',') {
                Some((a, b)) => (a.parse().ok()?, b.parse().ok()?),
                None => {
                    let n: usize = range.parse().ok()?;
                    (n, n)
                }
            };
            if !words.iter().any(|w| w == "-n") || start == 0 || end < start {
                return None;
            }
            args.insert("path".into(), file.into());
            args.insert("offset".into(), start.into());
            args.insert("limit".into(), (end - start + 1).into());
        }
        "grep" | "rg" => {
            args.insert("op".into(), "search".into());
            let mut positional = words[1..].iter().peekable();
            let mut pattern = None;
            let mut extended = false;
            while let Some(word) = positional.next() {
                if let Some(flag) = word.strip_prefix('-') {
                    if flag.starts_with('-') {
                        match flag {
                            "-files-with-matches" => {
                                args.insert("files".into(), true.into());
                            }
                            "-count" => {
                                args.insert("count".into(), true.into());
                            }
                            "-ignore-case" => {
                                args.insert("caseSensitive".into(), false.into());
                            }
                            "-fixed-strings" => {
                                args.insert("literal".into(), true.into());
                            }
                            "-after-context" => {
                                args.insert("after".into(), next_num(&mut positional)?.into());
                            }
                            "-before-context" => {
                                args.insert("before".into(), next_num(&mut positional)?.into());
                            }
                            "-context" => {
                                args.insert("context".into(), next_num(&mut positional)?.into());
                            }
                            "-no-filename" | "-with-filename" | "-line-number"
                            | "-no-line-number" | "-smart-case" | "-hidden" | "-quiet" => {}
                            "-invert-match" | "-word-regexp" | "-line-regexp" => return None,
                            _other => return None,
                        }
                        continue;
                    }
                    let chars: Vec<char> = flag.chars().collect();
                    let mut i = 0;
                    while i < chars.len() {
                        match chars[i] {
                            'i' => {
                                args.insert("caseSensitive".into(), false.into());
                            }
                            'l' => {
                                args.insert("files".into(), true.into());
                            }
                            'c' => {
                                args.insert("count".into(), true.into());
                            }
                            'F' => {
                                args.insert("literal".into(), true.into());
                            }
                            'v' | 'w' | 'x' => return None,
                            'e' => {
                                let rest: String = chars[i + 1..].iter().collect();
                                pattern = if rest.is_empty() {
                                    positional.next().cloned()
                                } else {
                                    Some(rest)
                                };
                                break;
                            }
                            'A' | 'B' | 'C' => {
                                let rest: String = chars[i + 1..].iter().collect();
                                let n = if rest.is_empty() {
                                    next_num(&mut positional)?
                                } else {
                                    rest.parse().ok()?
                                };
                                let key = match chars[i] {
                                    'A' => "after",
                                    'B' => "before",
                                    _other => "context",
                                };
                                args.insert(key.into(), n.into());
                                break;
                            }
                            'E' => extended = true,
                            'n' | 'N' | 'r' | 'R' | 'S' | 'H' | 'h' | 's' | 'q' => {}
                            _other => return None,
                        }
                        i += 1;
                    }
                } else if pattern.is_none() {
                    pattern = Some(word.clone());
                } else if !args.contains_key("path") {
                    args.insert("path".into(), word.clone().into());
                } else {
                    return None;
                }
            }
            let bre = verb == "grep" && !extended && !args.contains_key("literal");
            let pattern = pattern.filter(|p| !bre || !p.contains(['{', '+']))?;
            args.insert("pattern".into(), pattern.into());
        }
        _other => return None,
    }
    Some((serde_json::Value::Object(args), pipe))
}

fn flag_value(words: &[String], flag: &str) -> Option<usize> {
    words
        .iter()
        .position(|w| w == flag)
        .and_then(|i| words.get(i + 1))
        .and_then(|w| w.parse().ok())
}

fn next_num<'a>(positional: &mut impl Iterator<Item = &'a String>) -> Option<usize> {
    positional.next()?.parse().ok()
}

const READ_VERBS: [&str; 33] = [
    "cat", "ls", "grep", "rg", "head", "tail", "wc", "echo", "printf", "pwd", "which", "env",
    "printenv", "type", "file", "stat", "du", "df", "date", "true", "false", "test", "tree",
    "less", "more", "sort", "uniq", "diff", "cut", "awk", "jq", "xxd", "od",
];

const GIT_READS: [&str; 8] = [
    "status",
    "log",
    "diff",
    "show",
    "branch",
    "ls-files",
    "blame",
    "rev-parse",
];

pub(super) fn readonly_verb(command: &str) -> bool {
    crate::policy::split_segments(command)
        .iter()
        .all(|segment| {
            let words = crate::policy::argv(segment);
            let args = words.get(1..).unwrap_or_default();
            let flag = |flags: &[&str]| {
                args.iter()
                    .any(|arg| flags.iter().any(|f| arg.starts_with(f)))
            };
            match crate::policy::verb(&words).as_deref() {
                None => true,
                Some("sed") => !flag(&["-i", "--in-place"]),
                Some("find") => !flag(&["-delete", "-exec", "-ok"]),
                Some("git") => args
                    .first()
                    .is_some_and(|sub| GIT_READS.contains(&sub.as_str())),
                Some(verb) => READ_VERBS.contains(&verb),
            }
        })
}
