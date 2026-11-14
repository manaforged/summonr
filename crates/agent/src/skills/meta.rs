use std::path::Path;

use summonr_core::text::{Frontmatter, squash};

const NAME: &str = "name";
const DESCRIPTION: &str = "description";
const WHEN: &str = "when_to_use";
const HINT: &str = "argument-hint";
const ARGUMENTS: &str = "arguments";
const TOOLS: &str = "allowed-tools";
const MODEL: &str = "model";
const INHERIT: &str = "inherit";
const MODEL_OFF: &str = "disable-model-invocation";
const USER_ON: &str = "user-invocable";
const TRUE: [&str; 4] = ["true", "yes", "on", "1"];
const FALSE: [&str; 4] = ["false", "no", "off", "0"];
const LISTING_MAX: usize = 1536;
const HEADING_MAX: usize = 80;

/// The frontmatter fields of a skill or command file, parsed once.
#[derive(Clone, Debug)]
pub(crate) struct Meta {
    pub(crate) name: Option<String>,
    pub(crate) about: String,
    pub(crate) hint: String,
    pub(crate) arguments: Vec<String>,
    pub(crate) allowed_tools: Vec<String>,
    pub(crate) model: Option<String>,
    pub(crate) model_invocable: bool,
    pub(crate) user_invocable: bool,
}

impl Meta {
    /// The fields of `file`; a missing or unreadable file has none.
    pub(crate) fn read(file: &Path) -> Self {
        let bytes = std::fs::read(file).unwrap_or_default();
        Self::parse(&Frontmatter::parse(&String::from_utf8_lossy(&bytes)))
    }

    /// The fields of parsed front matter, with the body's first line as the fallback summary.
    pub(crate) fn parse(front: &Frontmatter<'_>) -> Self {
        let about: Vec<String> = [front.get(DESCRIPTION), front.get(WHEN)]
            .into_iter()
            .flatten()
            .map(squash)
            .collect();
        let about = if about.is_empty() {
            heading(front.body)
        } else {
            about.join(" ").chars().take(LISTING_MAX).collect()
        };
        Self {
            name: front.get(NAME).map(str::to_owned),
            about,
            hint: front.get(HINT).unwrap_or_default().to_owned(),
            arguments: values(front, ARGUMENTS),
            allowed_tools: values(front, TOOLS),
            model: front
                .get(MODEL)
                .filter(|model| *model != INHERIT)
                .map(str::to_owned),
            model_invocable: !flag(front, MODEL_OFF, false),
            user_invocable: flag(front, USER_ON, true),
        }
    }

    /// The summary the `/` menu shows, followed by the argument hint.
    pub(crate) fn offer(&self) -> String {
        if self.hint.is_empty() {
            return self.about.clone();
        }
        format!("{} {}", self.about, self.hint)
    }
}

fn heading(body: &str) -> String {
    body.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
        .trim_start_matches('#')
        .trim()
        .chars()
        .take(HEADING_MAX)
        .collect()
}

/// The boolean `key` sets, or `default` when it is unset or not a boolean word.
pub(crate) fn flag(front: &Frontmatter<'_>, key: &str, default: bool) -> bool {
    let Some(value) = front.get(key).map(str::to_ascii_lowercase) else {
        return default;
    };
    if TRUE.contains(&value.as_str()) {
        true
    } else if FALSE.contains(&value.as_str()) {
        false
    } else {
        default
    }
}

fn values(front: &Frontmatter<'_>, key: &str) -> Vec<String> {
    match front.get(key) {
        Some(value) if !value.starts_with('[') => split(value),
        Some(_) | None => front.list(key).unwrap_or_default(),
    }
}

fn split(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut depth = 0_usize;
    for ch in value.chars() {
        if depth == 0 && matches!(ch, ',' | ' ' | '\t') {
            if !word.is_empty() {
                out.push(std::mem::take(&mut word));
            }
            continue;
        }
        if ch == '(' {
            depth += 1;
        } else if ch == ')' {
            depth = depth.saturating_sub(1);
        }
        word.push(ch);
    }
    if !word.is_empty() {
        out.push(word);
    }
    out
}
