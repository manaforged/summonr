use std::path::Path;

use summonr_core::text::{Frontmatter, squash};

const NAME: &str = "name";
const DESCRIPTION: &str = "description";
const WHEN: &str = "when_to_use";
const HINT: &str = "argument-hint";
const ARGUMENTS: &str = "arguments";
const MODEL: &str = "model";
const INHERIT: &str = "inherit";
const UNAPPLIED: [&str; 10] = [
    "allowed-tools",
    "disallowed-tools",
    MODEL,
    "effort",
    "context",
    "agent",
    "background",
    "hooks",
    "paths",
    "shell",
];
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
    pub(crate) unapplied: Vec<&'static str>,
    pub(crate) model_invocable: bool,
    pub(crate) user_invocable: bool,
}

impl Meta {
    /// The fields of `file`, read from its text up to the first byte that is not UTF-8.
    pub(crate) fn read(file: &Path) -> std::io::Result<Self> {
        let bytes = std::fs::read(file)?;
        let text = std::str::from_utf8(&bytes).unwrap_or_else(|error| {
            std::str::from_utf8(&bytes[..error.valid_up_to()]).unwrap_or_default()
        });
        Ok(Self::parse(&Frontmatter::parse(text)))
    }

    /// The fields of `file`; an unreadable file has none and prints one note naming the file and the error.
    pub(crate) fn read_or_note(file: &Path) -> Self {
        Self::read(file).unwrap_or_else(|error| {
            let problem = crate::plugins::located(file, error);
            crate::display::note(&format!("{problem}; summonr cannot read this skill"));
            Self::parse(&Frontmatter::parse(""))
        })
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
            unapplied: UNAPPLIED
                .into_iter()
                .filter(|key| front.list(key).is_some())
                .filter(|key| *key != MODEL || front.get(MODEL) != Some(INHERIT))
                .collect(),
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

    /// Records that the file sets `key`, which summonr reads but does not apply.
    pub(crate) fn skip(&mut self, key: &'static str) {
        if !self.unapplied.contains(&key) {
            self.unapplied.push(key);
        }
    }

    /// One note per key in `unapplied`, for the `kind` named `name`.
    pub(crate) fn notes(&self, kind: &str, name: &str) -> Vec<String> {
        self.unapplied
            .iter()
            .map(|key| unapplied(kind, name, key))
            .collect()
    }
}

/// The note for `field` of the `kind` named `name`, which summonr reads but does not apply.
pub(crate) fn unapplied(kind: &str, name: &str, field: &str) -> String {
    format!("{kind} {name} {field} (summonr does not apply it)")
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
    front.names(key).unwrap_or_default()
}
