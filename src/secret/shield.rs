use std::path::Path;

const DEPTH: usize = 3;
const MIN_LEN: usize = 6;
const LONG: usize = 16;
const SECRET_WORDS: &[&str] = &[
    "KEY",
    "APIKEY",
    "SECRET",
    "TOKEN",
    "AUTHTOKEN",
    "PASS",
    "PASSWD",
    "PASSWORD",
    "PWD",
    "AUTH",
    "CREDENTIAL",
    "CREDENTIALS",
    "PRIVATE",
    "DSN",
    "COOKIE",
    "SALT",
    "SIGNATURE",
];
const HOME_FILES: &[&str] = &[
    ".aws/credentials",
    ".netrc",
    ".npmrc",
    ".pypirc",
    ".git-credentials",
    ".config/gh/hosts.yml",
    ".docker/config.json",
];
const WORKSPACE_FILES: &[&str] = &[".npmrc", ".pypirc"];
const KEY_BEGIN: &str = "-----BEGIN ";
const KEY_END: &str = "-----END ";
const KEY_TAIL: &str = "PRIVATE KEY-----";
const HIDDEN: &str = "[secret]";

pub(crate) fn scrub(text: &str, workspace: &Path) -> String {
    let text = crate::askpass::scrub(text);
    let values = values(workspace);
    let text = crate::text::redact(&text, values.iter().map(String::as_str));
    private_keys(&text)
}

fn values(workspace: &Path) -> Vec<String> {
    let mut found: Vec<String> = files(workspace)
        .iter()
        .filter_map(|path| std::fs::read_to_string(path).ok())
        .flat_map(|text| secrets_in(&text))
        .collect();
    found.sort_by_key(|value| std::cmp::Reverse(value.len()));
    found.dedup();
    found
}

fn files(workspace: &Path) -> Vec<std::path::PathBuf> {
    let home = crate::config::home_dir();
    let mut paths: Vec<std::path::PathBuf> = HOME_FILES
        .iter()
        .filter_map(|name| home.as_ref().map(|home| home.join(name)))
        .chain(WORKSPACE_FILES.iter().map(|name| workspace.join(name)))
        .collect();
    let walk = ignore::WalkBuilder::new(workspace)
        .standard_filters(false)
        .max_depth(Some(DEPTH))
        .filter_entry(|entry| {
            entry.depth() == 0
                || !entry.file_type().is_some_and(|kind| kind.is_dir())
                || !crate::SKIP_DIRS.contains(&entry.file_name().to_string_lossy().as_ref())
        })
        .build();
    paths.extend(walk.filter_map(Result::ok).filter_map(|entry| {
        let name = entry.file_name().to_string_lossy();
        (entry.file_type().is_some_and(|kind| kind.is_file())
            && (name == ".env" || name.starts_with(".env.")))
        .then(|| entry.into_path())
    }));
    paths
}

fn secrets_in(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut words = text.split_whitespace().peekable();
    while let Some(word) = words.next() {
        if let Some((_, rest)) = word.split_once("://")
            && rest
                .split('@')
                .next()
                .is_some_and(|user| user.contains(':'))
            && rest.contains('@')
        {
            found.push(word.to_string());
        }
        if word == "password"
            && let Some(value) = words.peek()
            && value.len() >= MIN_LEN
        {
            found.push((*value).to_string());
        }
    }
    found.extend(text.lines().filter_map(assignment));
    found
}

fn assignment(line: &str) -> Option<String> {
    let line = line.trim();
    let line = line.strip_prefix("export ").unwrap_or(line);
    if line.starts_with('#') {
        return None;
    }
    let (key, value) = line
        .split_once('=')
        .filter(|(key, _)| !key.contains(':'))
        .or_else(|| line.split_once(':'))?;
    let key = key.trim().trim_matches('"').to_ascii_uppercase();
    let value = unquoted(value.trim().trim_end_matches(','));
    let named = key
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .any(|word| SECRET_WORDS.contains(&word));
    let url = value.contains("://") && value.contains('@');
    let opaque = value.len() >= LONG
        && !value.contains(char::is_whitespace)
        && value.chars().any(|ch| ch.is_ascii_digit())
        && value.chars().any(|ch| ch.is_ascii_alphabetic());
    (value.len() >= MIN_LEN && (named || url || opaque)).then(|| value.to_string())
}

fn unquoted(value: &str) -> &str {
    for quote in ['"', '\''] {
        if let Some(inner) = value
            .strip_prefix(quote)
            .and_then(|rest| rest.split_once(quote))
            .map(|(inner, _)| inner)
        {
            return inner;
        }
    }
    value
        .split_once(" #")
        .map_or(value, |(value, _)| value)
        .trim()
}

fn private_keys(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(KEY_BEGIN) {
        let after = &rest[start..];
        let Some(head) = after.find('\n').map(|end| &after[..end]) else {
            break;
        };
        if !head.ends_with(KEY_TAIL) {
            out.push_str(&rest[..start + KEY_BEGIN.len()]);
            rest = &rest[start + KEY_BEGIN.len()..];
            continue;
        }
        out.push_str(&rest[..start]);
        out.push_str(HIDDEN);
        rest = after
            .find(KEY_END)
            .and_then(|end| {
                after[end..]
                    .find(KEY_TAIL)
                    .map(|tail| &after[end + tail + KEY_TAIL.len()..])
            })
            .unwrap_or("");
    }
    out.push_str(rest);
    out
}
