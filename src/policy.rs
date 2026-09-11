use std::path::{Component, Path};

use serde::Deserialize;

use crate::error::{Error, Result};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Policy {
    pub strict: bool,
    read_only: bool,
    unrestricted: bool,
    deny: Vec<String>,
    allow: Vec<Vec<String>>,
    write: Vec<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
struct File {
    #[serde(default)]
    strict: bool,
    #[serde(default)]
    deny: Vec<String>,
    #[serde(default)]
    allow: Vec<String>,
    #[serde(default)]
    write: Vec<String>,
}

impl Policy {
    #[must_use]
    pub(crate) fn builtin() -> Self {
        Self::default()
    }

    pub(crate) fn discover(workspace: &Path) -> Result<Self> {
        let mut policy = Self::builtin();
        if let Ok(home) = crate::prompt::mage_home() {
            policy.overlay_file(&home.join("policy.toml"))?;
        }
        policy.overlay_file(&workspace.join(".mage/policy.toml"))?;
        Ok(policy)
    }

    fn overlay_file(&mut self, path: &Path) -> Result<()> {
        if !path.is_file() {
            return Ok(());
        }
        let text = std::fs::read_to_string(path)?;
        let file: File = toml::from_str(&text)
            .map_err(|error| Error::Config(format!("{}: {error}", path.display())))?;
        self.overlay(file);
        Ok(())
    }

    fn overlay(&mut self, file: File) {
        self.strict |= file.strict;
        for item in file.deny {
            if !item.trim().is_empty() && !self.deny.iter().any(|have| have == &item) {
                self.deny.push(item);
            }
        }
        if file.strict || !file.allow.is_empty() {
            self.allow.push(file.allow);
        }
        if !file.write.is_empty() {
            self.write.push(file.write);
        }
    }

    pub(crate) fn set_strict(&mut self) {
        self.unrestricted = false;
        self.strict = true;
    }

    pub(crate) fn set_unrestricted(&mut self) {
        self.unrestricted = true;
    }

    #[must_use]
    pub(crate) fn is_unrestricted(&self) -> bool {
        self.unrestricted
    }

    #[must_use]
    pub(crate) fn restricts_writes(&self) -> bool {
        !self.unrestricted && !self.write.is_empty()
    }

    pub(crate) fn set_read_only(&mut self, on: bool) {
        self.read_only = on;
    }

    #[must_use]
    pub(crate) fn is_read_only(&self) -> bool {
        self.read_only
    }

    pub(crate) fn command(&self, command: &str, workspace: &Path) -> Result<()> {
        let command = command.trim();
        if command.is_empty() {
            return Err(Error::Denied("command is empty".to_string()));
        }
        if self.unrestricted {
            return Ok(());
        }
        if builtin_deny(command) {
            return Err(Error::Denied(format!("blocked command `{command}`")));
        }
        if let Some(path) = escapes(command, workspace) {
            return Err(Error::Denied(format!(
                "`{path}` is outside the workspace — commands may only change files under it"
            )));
        }
        for pattern in &self.deny {
            if matches_deny(command, pattern) {
                return Err(Error::Denied(format!("denied by policy `{pattern}`")));
            }
        }
        if (self.strict && self.allow.is_empty())
            || self
                .allow
                .iter()
                .any(|list| !list.iter().any(|pattern| matches_allow(command, pattern)))
        {
            return Err(Error::Denied(format!(
                "command `{command}` is not on every configured allow list"
            )));
        }
        Ok(())
    }

    pub(crate) fn write_path(&self, relative: &str) -> Result<()> {
        if self.read_only {
            return Err(Error::Denied(
                "plan mode does not edit files. Write a new plan or review with the write_file \
                 tool, or propose the change as a diff."
                    .to_string(),
            ));
        }
        self.write_globs(relative)
    }

    pub(crate) fn write_globs(&self, relative: &str) -> Result<()> {
        if self.unrestricted {
            return Ok(());
        }
        if self.write.is_empty() {
            return Ok(());
        }
        let relative = relative.replace('\\', "/");
        if self
            .write
            .iter()
            .all(|list| list.iter().any(|glob| path_matches(&relative, glob)))
        {
            return Ok(());
        }
        Err(Error::Denied(format!(
            "write to `{relative}` is not in every configured write list"
        )))
    }
}

fn expands_to_shell(var: &str) -> bool {
    let name: String = var
        .trim_start_matches('{')
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    if name.is_empty() {
        return true;
    }
    match std::env::var(&name) {
        Ok(value) => shell(&value),
        Err(_) => true,
    }
}

fn shell(program: &str) -> bool {
    let name = program
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(program)
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .trim_matches(['\'', '"'])
        .to_ascii_lowercase();
    matches!(
        name.strip_suffix(".exe").unwrap_or(&name),
        "sh" | "bash" | "zsh" | "dash" | "ksh" | "fish" | "cmd" | "powershell" | "pwsh"
    )
}

fn pipes_to_shell(command: &str) -> bool {
    unquoted(command, &["|"])
        .into_iter()
        .skip(1)
        .any(|segment| {
            segment
                .trim_start_matches('&')
                .split_whitespace()
                .map(|word| word.rsplit('/').next().unwrap_or(word))
                .find(|word| {
                    !matches!(
                        *word,
                        "env"
                            | "sudo"
                            | "doas"
                            | "nice"
                            | "nohup"
                            | "exec"
                            | "command"
                            | "time"
                            | "xargs"
                            | "timeout"
                            | "stdbuf"
                            | "setsid"
                            | "flock"
                            | "chronic"
                    ) && !word.starts_with('-')
                        && !word.contains('=')
                        && !word.chars().all(|c| c.is_ascii_digit())
                })
                .is_some_and(|word| {
                    let bare: String = word.chars().filter(|c| !matches!(c, '"' | '\'')).collect();
                    if let Some(var) = bare.strip_prefix('$') {
                        return expands_to_shell(var);
                    }
                    let name = bare
                        .trim_end_matches(|c: char| !c.is_ascii_alphanumeric())
                        .to_ascii_lowercase();
                    shell(&name) || word.starts_with('`')
                })
        })
}

fn builtin_deny(command: &str) -> bool {
    let folded = collapse(command);
    wipes_root(&folded)
        || folded.contains("git push --force")
        || folded.contains("git push -f")
        || folded.contains("git push --mirror")
        || pipes_to_shell(command)
        || folded.contains("mkfs")
        || folded.contains(":(){ :|:& };:")
        || folded.starts_with("sudo ")
        || folded.contains(" sudo ")
        || powers_off(&folded)
        || folded.contains("chmod -r 777 /")
        || (folded.contains("dd if=") && folded.contains("of=/dev/"))
}

fn powers_off(folded: &str) -> bool {
    split_segments(folded).iter().any(|segment| {
        segment
            .split_whitespace()
            .find(|word| !word.contains('='))
            .map(|word| word.rsplit('/').next().unwrap_or(word))
            .is_some_and(|verb| matches!(verb, "shutdown" | "reboot" | "halt" | "poweroff"))
    })
}

const DESTRUCTIVE: [&str; 14] = [
    "rm", "rmdir", "unlink", "mv", "cp", "chmod", "chown", "truncate", "tee", "dd", "ln", "shred",
    "install", "rsync",
];

fn escapes(command: &str, workspace: &Path) -> Option<String> {
    let mut outside_cwd = false;
    for segment in split_segments(command) {
        let segment = if outside_cwd || !workspace.join(".git").exists() {
            segment
        } else {
            segment.replace("$(git rev-parse --show-toplevel)", ".")
        };
        let words: Vec<String> = segment
            .split_whitespace()
            .map(|word| word.trim_matches(|c| c == '"' || c == '\'').to_string())
            .filter(|word| !word.is_empty())
            .collect();
        let Some(verb) = words.iter().find(|word| !word.contains('=')) else {
            continue;
        };
        let verb = verb.rsplit('/').next().unwrap_or(verb).to_string();
        let args = words
            .iter()
            .skip_while(|word| {
                word.contains('=') || **word != verb && !word.ends_with(&format!("/{verb}"))
            })
            .skip(1);
        let deletes_via_find = verb == "find"
            && words
                .iter()
                .any(|word| word == "-delete" || word == "-exec" || word == "-execdir");
        let destructive = DESTRUCTIVE.contains(&verb.as_str()) || deletes_via_find;
        for word in args.clone() {
            if let Some(target) = word.strip_prefix(">>").or_else(|| word.strip_prefix('>')) {
                let target = if target.is_empty() { continue } else { target };
                if is_outside(target, workspace, outside_cwd) {
                    return Some(target.to_string());
                }
            }
        }
        let mut redirect_next = false;
        let mut targets: Vec<String> = Vec::new();
        for word in args.clone() {
            if redirect_next {
                redirect_next = false;
                if is_outside(word, workspace, outside_cwd) {
                    return Some(word.clone());
                }
                continue;
            }
            if word == ">" || word == ">>" || word == "1>" || word == "2>" || word == "&>" {
                redirect_next = true;
                continue;
            }
            if word.starts_with('-') {
                continue;
            }
            if verb == "cd" {
                if is_outside(word, workspace, outside_cwd) {
                    outside_cwd = true;
                }
                break;
            }
            if destructive {
                if in_temp(word) {
                    return Some(word.clone());
                }
                targets.push(word.clone());
            }
        }
        let checked: Vec<&String> = if DEST_ONLY.contains(&verb.as_str()) {
            targets.last().into_iter().collect()
        } else {
            targets.iter().collect()
        };
        for word in checked {
            if outside_cwd || is_outside(word, workspace, false) {
                return Some(word.clone());
            }
        }
    }
    None
}

const DEST_ONLY: [&str; 5] = ["cp", "mv", "ln", "install", "rsync"];

fn split_segments(command: &str) -> Vec<String> {
    unquoted(command, &["&&", "||", "|", ";"])
        .into_iter()
        .map(|segment| segment.trim().to_string())
        .filter(|segment| !segment.is_empty())
        .collect()
}

fn unquoted(command: &str, delimiters: &[&str]) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut open: Vec<char> = Vec::new();
    let mut rest = command;
    while let Some(ch) = rest.chars().next() {
        let mut take = ch.len_utf8();
        match open.last() {
            Some('\'') => {
                if ch == '\'' {
                    open.pop();
                }
            }
            Some('"') => {
                if ch == '\\' {
                    take += rest[take..].chars().next().map_or(0, char::len_utf8);
                } else if ch == '"' {
                    open.pop();
                } else if rest.starts_with("$(") {
                    open.push('(');
                    take = 2;
                }
            }
            _ => {
                if ch == '\\' {
                    take += rest[take..].chars().next().map_or(0, char::len_utf8);
                } else if ch == '\'' || ch == '"' {
                    open.push(ch);
                } else if rest.starts_with("$(") {
                    open.push('(');
                    take = 2;
                } else if ch == ')' && open.last() == Some(&'(') {
                    open.pop();
                } else if let Some(delimiter) = delimiters.iter().find(|d| rest.starts_with(*d)) {
                    segments.push(std::mem::take(&mut current));
                    rest = &rest[delimiter.len()..];
                    continue;
                }
            }
        }
        current.push_str(&rest[..take]);
        rest = &rest[take..];
    }
    segments.push(current);
    segments
}

fn in_temp(word: &str) -> bool {
    let temp = std::env::temp_dir();
    let target = Path::new(word.trim_end_matches('*'));
    target.has_root()
        && [temp.clone(), temp.canonicalize().unwrap_or_default()]
            .iter()
            .any(|root| !root.as_os_str().is_empty() && target.starts_with(root))
}

fn is_outside(word: &str, workspace: &Path, outside_cwd: bool) -> bool {
    if matches!(word, "/dev/null" | "/dev/stdout" | "/dev/stderr") {
        return false;
    }
    if word.starts_with('~') || word.starts_with('$') {
        return true;
    }
    if Path::new(word).has_root() {
        let target = Path::new(word);
        let temp = std::env::temp_dir();
        let roots = [
            workspace.to_path_buf(),
            workspace.canonicalize().unwrap_or_default(),
            temp.clone(),
            temp.canonicalize().unwrap_or_default(),
        ];
        return !roots
            .iter()
            .any(|root| !root.as_os_str().is_empty() && target.starts_with(root));
    }
    if outside_cwd {
        return true;
    }
    let mut depth: i64 = 0;
    for part in Path::new(word).components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                depth -= 1;
                if depth < 0 {
                    return true;
                }
            }
            Component::Prefix(_) => return true,
            _ => depth += 1,
        }
    }
    false
}

fn wipes_root(folded: &str) -> bool {
    ["rm -rf /", "rm -fr /", "rm -r /"].iter().any(|prefix| {
        folded.match_indices(prefix).any(|(index, _)| {
            matches!(
                folded[index + prefix.len()..].chars().next(),
                None | Some(' ') | Some('*')
            )
        })
    })
}

fn collapse(command: &str) -> String {
    let mut out = String::with_capacity(command.len());
    let mut space = false;
    for char in command.to_ascii_lowercase().chars() {
        if char.is_whitespace() {
            if !space && !out.is_empty() {
                out.push(' ');
                space = true;
            }
        } else {
            space = false;
            out.push(char);
        }
    }
    out
}

fn matches_allow(command: &str, pattern: &str) -> bool {
    let command = collapse(command);
    let pattern = collapse(pattern);
    if pattern.is_empty() {
        return false;
    }
    command == pattern || command.starts_with(&format!("{pattern} "))
}

fn matches_deny(command: &str, pattern: &str) -> bool {
    let command = collapse(command);
    let pattern = collapse(pattern);
    if pattern.is_empty() {
        return false;
    }
    command == pattern || command.starts_with(&format!("{pattern} ")) || command.contains(&pattern)
}

fn path_matches(path: &str, glob: &str) -> bool {
    let glob = glob.replace('\\', "/");
    if let Some(prefix) = glob.strip_suffix("/**") {
        return path == prefix || path.starts_with(&format!("{prefix}/"));
    }
    if let Some(suffix) = glob.strip_prefix("*.") {
        return path
            .rsplit('/')
            .next()
            .is_some_and(|name| name.ends_with(&format!(".{suffix}")));
    }
    if let Some(prefix) = glob.strip_suffix('/') {
        return path == prefix || path.starts_with(&format!("{prefix}/"));
    }
    path == glob || path.starts_with(&format!("{glob}/"))
}

#[cfg(test)]
#[path = "policy_tests.rs"]
mod tests;
