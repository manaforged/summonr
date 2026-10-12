use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::SystemTime;

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
const ENV_FILE: &str = ".env";
const KEY_BEGIN: &str = "-----BEGIN ";
const KEY_END: &str = "-----END ";
const KEY_TAIL: &str = "PRIVATE KEY-----";
const HIDDEN: &str = "[secret]";
const TOKENS: &str = include_str!("tokens.toml");

#[derive(serde::Deserialize, Clone, Copy, Default)]
#[serde(rename_all = "lowercase")]
enum Chars {
    #[default]
    Token,
    Upper,
}

impl Chars {
    fn allows(self, c: char) -> bool {
        match self {
            Self::Token => c.is_ascii_alphanumeric() || matches!(c, '_' | '-'),
            Self::Upper => c.is_ascii_uppercase() || c.is_ascii_digit(),
        }
    }
}

#[derive(serde::Deserialize)]
struct Shape {
    prefix: String,
    min: usize,
    #[serde(default)]
    chars: Chars,
}

#[derive(serde::Deserialize)]
struct Shapes {
    token: Vec<Shape>,
}

type Stamp = Option<(SystemTime, u64)>;

struct Scan {
    watched: Vec<(PathBuf, Stamp)>,
    values: Arc<Vec<String>>,
}

static SEEN: Mutex<Option<HashMap<PathBuf, Scan>>> = Mutex::new(None);

pub(crate) fn scrub(text: &str, workspace: &Path) -> String {
    let text = crate::askpass::scrub(text);
    let values = known(workspace);
    let text = crate::text::redact(&text, values.iter().map(String::as_str));
    private_keys(&text)
}

pub(crate) fn scrub_found(text: &str, workspace: &Path) -> String {
    let found = secrets_in(text);
    let text = crate::text::redact(text, found.iter().map(String::as_str));
    scrub(&text, workspace)
}

fn known(workspace: &Path) -> Arc<Vec<String>> {
    let mut seen = SEEN.lock().unwrap_or_else(PoisonError::into_inner);
    let seen = seen.get_or_insert_with(HashMap::new);
    if let Some(scan) = seen.get(workspace)
        && scan.watched.iter().all(|(path, was)| stamp(path) == *was)
    {
        return Arc::clone(&scan.values);
    }
    let (files, folders) = files(workspace);
    let watched: Vec<(PathBuf, Stamp)> = files
        .iter()
        .chain(&folders)
        .map(|path| (path.clone(), stamp(path)))
        .collect();
    let values = Arc::new(values(&files));
    seen.insert(
        workspace.to_path_buf(),
        Scan {
            watched,
            values: Arc::clone(&values),
        },
    );
    values
}

fn stamp(path: &Path) -> Stamp {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

fn values(files: &[PathBuf]) -> Vec<String> {
    let mut found: Vec<String> = files
        .iter()
        .filter_map(|path| std::fs::read_to_string(path).ok())
        .flat_map(|text| secrets_in(&text))
        .collect();
    found.sort_by_key(|value| std::cmp::Reverse(value.len()));
    found.dedup();
    found
}

fn files(workspace: &Path) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let home = crate::config::home_dir();
    let mut paths: Vec<PathBuf> = HOME_FILES
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
    let mut folders = Vec::new();
    for entry in walk.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy();
        if entry.file_type().is_some_and(|kind| kind.is_dir()) {
            folders.push(entry.into_path());
        } else if env_file(&name) {
            paths.push(entry.into_path());
        }
    }
    (paths, folders)
}

fn env_file(name: &str) -> bool {
    name.strip_prefix(ENV_FILE)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
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
    found.extend(tokens(text));
    found
}

fn shapes() -> &'static [Shape] {
    static LOADED: OnceLock<Shapes> = OnceLock::new();
    &LOADED
        .get_or_init(|| match toml::from_str(TOKENS) {
            Ok(shapes) => shapes,
            Err(error) => panic!("the token shapes table is invalid: {error}"),
        })
        .token
}

fn tokens(text: &str) -> Vec<String> {
    shapes()
        .iter()
        .flat_map(|shape| {
            text.match_indices(shape.prefix.as_str())
                .filter_map(move |(start, _)| token_at(text, start, shape))
        })
        .collect()
}

fn token_at(text: &str, start: usize, shape: &Shape) -> Option<String> {
    let after = start + shape.prefix.len();
    let boundary = text[..start]
        .chars()
        .next_back()
        .is_none_or(|c| !Chars::Token.allows(c));
    let run = text[after..]
        .find(|c: char| !Chars::Token.allows(c))
        .map_or(text.len(), |end| after + end);
    let body = &text[after..run];
    (boundary && body.len() >= shape.min && body.chars().all(|c| shape.chars.allows(c)))
        .then(|| text[start..run].to_string())
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

#[cfg(test)]
#[path = "shield_tests.rs"]
mod tests;
