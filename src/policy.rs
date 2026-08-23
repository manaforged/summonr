//! Allow / deny for shell and writes. Stricter overlay wins.

use std::path::Path;

use serde::Deserialize;

use crate::error::{Error, Result};

/// Tool permission set for one Summoner process.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Policy {
    /// When true, only `allow` commands run.
    pub strict: bool,
    /// When true, every command and write is denied (`summon --plan`).
    /// An allow list or write list cannot widen this.
    read_only: bool,
    deny: Vec<String>,
    allow: Vec<String>,
    write: Vec<String>,
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
    /// Built-in deny list. Interactive and `-p` use this unless `strict`.
    #[must_use]
    pub(crate) fn builtin() -> Self {
        Self::default()
    }

    /// Overlay `$MAGE_HOME/policy.toml` then `<workspace>/.mage/policy.toml`.
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
        if !file.allow.is_empty() {
            self.allow = file.allow;
        }
        if !file.write.is_empty() {
            self.write = file.write;
        }
    }

    pub(crate) fn set_strict(&mut self) {
        self.strict = true;
    }

    /// Enter plan mode: read-only, fail closed, no overlay can lift it.
    /// Set plan mode. Only the runtime mode switch may clear it —
    /// policy-file overlays run before boot and can never lift it.
    pub(crate) fn set_read_only(&mut self, on: bool) {
        self.read_only = on;
    }

    #[must_use]
    pub(crate) fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Decide whether `command` may run inside `workspace`. Empty command
    /// is denied. A destructive verb or an output redirect that names a
    /// path outside the workspace is denied.
    pub(crate) fn command(&self, command: &str, workspace: &Path) -> Result<()> {
        let command = command.trim();
        if command.is_empty() {
            return Err(Error::Denied("command is empty".to_string()));
        }
        if self.read_only {
            return Err(Error::Denied(
                "plan mode is read-only — no commands run".to_string(),
            ));
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
        if self.strict || !self.allow.is_empty() {
            let allowed = self
                .allow
                .iter()
                .any(|pattern| matches_allow(command, pattern));
            if !allowed {
                return Err(Error::Denied(format!(
                    "command `{command}` is not on the allow list"
                )));
            }
        }
        Ok(())
    }

    /// Decide whether a workspace-relative path may be written.
    pub(crate) fn write_path(&self, relative: &str) -> Result<()> {
        if self.read_only {
            return Err(Error::Denied(
                "plan mode is read-only — no file writes".to_string(),
            ));
        }
        if self.write.is_empty() {
            return Ok(());
        }
        let relative = relative.replace('\\', "/");
        if self.write.iter().any(|glob| path_matches(&relative, glob)) {
            return Ok(());
        }
        Err(Error::Denied(format!(
            "write to `{relative}` is not in the write list"
        )))
    }
}

fn builtin_deny(command: &str) -> bool {
    let folded = collapse(command);
    wipes_root(&folded)
        || folded.contains("git push --force")
        || folded.contains("git push -f")
        || folded.contains("git push --mirror")
        || folded.contains("| sh")
        || folded.contains("| bash")
        || folded.contains("|sh")
        || folded.contains("|bash")
        || folded.contains("mkfs")
        || folded.contains(":(){ :|:& };:")
        || folded.starts_with("sudo ")
        || folded.contains(" sudo ")
        || folded.contains("shutdown")
        || folded.contains("reboot")
        || folded.contains("halt")
        || folded.contains("chmod -r 777 /")
        || (folded.contains("dd if=") && folded.contains("of=/dev/"))
}

/// Verbs that change or remove what they are pointed at.
const DESTRUCTIVE: [&str; 14] = [
    "rm", "rmdir", "unlink", "mv", "cp", "chmod", "chown", "truncate", "tee", "dd", "ln", "shred",
    "install", "rsync",
];

/// The first path named by a destructive verb, a `cd`, or an output
/// redirect that resolves outside `workspace`. `None` when the command
/// stays inside.
fn escapes(command: &str, workspace: &Path) -> Option<String> {
    let mut outside_cwd = false;
    for segment in split_segments(command) {
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
                targets.push(word.clone());
            }
        }
        // cp/mv/ln/install/rsync read their sources; only the destination
        // (last operand) changes anything.
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

/// Verbs whose only written operand is the last one.
const DEST_ONLY: [&str; 5] = ["cp", "mv", "ln", "install", "rsync"];

/// Split on `;`, `&&`, `||`, `|`, and newlines. Quotes are not tracked;
/// a false split only makes the check stricter.
fn split_segments(command: &str) -> Vec<String> {
    command
        .replace("&&", "\n")
        .replace("||", "\n")
        .replace(['|', ';'], "\n")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

/// True when `word` names a path that is not under `workspace`. Device
/// sinks are allowed. Relative paths are resolved lexically; `..` that
/// climbs past the workspace root is outside.
fn is_outside(word: &str, workspace: &Path, outside_cwd: bool) -> bool {
    if matches!(word, "/dev/null" | "/dev/stdout" | "/dev/stderr") {
        return false;
    }
    if word.starts_with('~') || word.starts_with('$') {
        return true;
    }
    if word.starts_with('/') {
        let target = Path::new(word);
        let roots = [
            workspace.to_path_buf(),
            workspace.canonicalize().unwrap_or_default(),
        ];
        return !roots
            .iter()
            .any(|root| !root.as_os_str().is_empty() && target.starts_with(root));
    }
    if outside_cwd {
        return true;
    }
    let mut depth: i64 = 0;
    for part in word.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                depth -= 1;
                if depth < 0 {
                    return true;
                }
            }
            _ => depth += 1,
        }
    }
    false
}

/// `rm -r` aimed at the filesystem root itself (`/`, `/*`), not at an
/// absolute path below it — those are judged by the workspace check.
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
