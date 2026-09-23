use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

use crate::error::{Error, Result};

mod danger;
pub(crate) use danger::{argv, danger, verb};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Policy {
    pub strict: bool,
    read_only: bool,
    unrestricted: bool,
    deny: Vec<String>,
    allow: Vec<Vec<String>>,
    write: Vec<Vec<String>>,
    roots: Vec<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Grant {
    Write(PathBuf),
    Guarded(PathBuf),
    Command(String),
    Danger(String),
    Edit(&'static str),
    Network,
}

impl Grant {
    #[must_use]
    pub(crate) fn prompt(&self) -> String {
        match self {
            Self::Write(path) => path.display().to_string(),
            Self::Guarded(path) => format!("protected path {}", path.display()),
            Self::Command(command) => format!("a shell command without the sandbox: {command}"),
            Self::Danger(command) => format!("a destructive command: {command}"),
            Self::Edit(setting) => format!("allow {setting} for this task (saved setting is off)"),
            Self::Network => "network".to_string(),
        }
    }
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
        if let Ok(home) = crate::prompt::summonr_home() {
            policy.overlay_file(&home.join("policy.toml"))?;
        }
        policy.overlay_file(&workspace.join(".summonr/policy.toml"))?;
        if let Ok(config) = crate::config::load() {
            policy.roots = config.sandbox.paths();
        }
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

    pub(crate) fn allow_path(&mut self, path: PathBuf) {
        let resolved = path.canonicalize().unwrap_or(path);
        if !self.roots.contains(&resolved) {
            self.roots.push(resolved);
        }
    }

    #[must_use]
    pub(crate) fn write_roots(&self) -> Vec<PathBuf> {
        self.roots.clone()
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
        self.command_policy(command)?;
        if self.unrestricted {
            return Ok(());
        }
        match write_block(command.trim(), workspace, &self.roots) {
            Some(Block::Protected(path)) => {
                return Err(Error::Denied(format!("`{path}` is a protected path")));
            }
            Some(Block::Outside(path)) => {
                return Err(Error::Denied(format!(
                    "`{path}` is outside the workspace — {}",
                    outside_hint(&path)
                )));
            }
            None => {}
        }
        Ok(())
    }

    pub(crate) fn command_policy(&self, command: &str) -> Result<()> {
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
        for pattern in &self.deny {
            if matches_deny(command, pattern) {
                return Err(Error::Denied(format!("denied by policy `{pattern}`")));
            }
        }
        if (self.strict && self.allow.is_empty())
            || !split_segments(command).iter().all(|segment| {
                !(self.strict && segment.contains(['`', '$', '<', '>'].as_ref()))
                    && (self.allow.is_empty()
                        || !["$(", "`", "<(", ">("]
                            .iter()
                            .any(|token| segment.contains(token)))
                    && self
                        .allow
                        .iter()
                        .all(|list| list.iter().any(|pattern| matches_allow(segment, pattern)))
            })
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
                "plan mode does not edit files. Return the plan in your reply. \
                 Switch to auto mode before editing files."
                    .to_string(),
            ));
        }
        if !self.unrestricted && protected_write(Path::new(relative)) {
            return Err(Error::Denied(format!("`{relative}` is a protected path")));
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
            "write to `{relative}` is not permitted by the policy write globs"
        )))
    }
}

#[must_use]
pub(crate) fn write_directory(path: &str) -> PathBuf {
    let trimmed = path.trim_end_matches(['/', '\\']);
    let directory =
        trimmed.rsplit_once(['/', '\\']).map_or(
            trimmed,
            |(head, _)| {
                if head.is_empty() { "/" } else { head }
            },
        );
    expand_path(directory)
}

#[must_use]
pub(crate) fn write_denied(path: &str) -> String {
    let dir = write_directory(path);
    format!(
        "write to `{path}` is outside the writable roots — add {} to [sandbox] write in ~/.summonr/config.toml, or /write {}",
        dir.display(),
        dir.display()
    )
}

#[must_use]
pub(crate) fn outside_hint(path: &str) -> String {
    write_denied(path)
}

pub(crate) fn grant_from_error(error: &Error) -> Option<Grant> {
    match error {
        Error::EditApproval { setting, .. } => Some(Grant::Edit(setting)),
        Error::Denied(message) | Error::Tool(message) => grant_from_message(message),
        _ => None,
    }
}

pub(crate) fn grant_from_message(message: &str) -> Option<Grant> {
    let rest = message.split_once("/write ")?.1;
    let dir = rest.split([',', '\n']).next()?.trim();
    if dir.is_empty() || dir == "PATH" {
        return None;
    }
    write_grant(expand_path(dir))
}

pub(crate) fn outside_write(command: &str, workspace: &Path, roots: &[PathBuf]) -> Option<Grant> {
    match write_block(command, workspace, roots) {
        Some(Block::Outside(path)) => write_grant(write_directory(&path)),
        Some(Block::Protected(path)) => {
            write_grant(write_directory(&locate(&path, workspace).to_string_lossy()))
        }
        None => None,
    }
}

fn write_grant(path: PathBuf) -> Option<Grant> {
    if !(path.is_absolute() || path.has_root()) {
        return None;
    }
    Some(if protected_write(&path) {
        Grant::Guarded(path)
    } else {
        Grant::Write(path)
    })
}

pub(crate) fn implicit_write(command: &str) -> Option<PathBuf> {
    let verb = first_verb(command)?;
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)?;
    let dir = match verb.as_str() {
        "cargo" | "rustup" => home.join(".cargo"),
        "npm" | "npx" | "pnpm" | "yarn" | "bun" => home.join(".npm"),
        "go" => std::env::var_os("GOPATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("go")),
        _ => return None,
    };
    (!protected_write(&dir)).then_some(dir)
}

pub(crate) fn first_verb(command: &str) -> Option<String> {
    let word = command
        .split_whitespace()
        .find(|word| !word.contains('='))?;
    let name = word
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(word)
        .trim_matches(['\'', '"', '`'])
        .to_ascii_lowercase();
    Some(name.strip_suffix(".exe").unwrap_or(&name).to_string())
}

pub(crate) fn protected_write(path: &Path) -> bool {
    let unix = path.to_string_lossy().replace('\\', "/");
    let parts: Vec<&str> = unix.split('/').filter(|part| !part.is_empty()).collect();
    if parts
        .windows(2)
        .any(|pair| pair[0] == ".git" && matches!(pair[1], "hooks" | "config"))
    {
        return true;
    }
    if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
        if name == ".env" || name.starts_with(".env.") {
            return true;
        }
        if matches!(
            name,
            ".bashrc" | ".zshrc" | ".zprofile" | ".profile" | ".bash_profile"
        ) {
            return true;
        }
    }
    let Some(home) = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
    else {
        return false;
    };
    [".ssh", ".gnupg", ".summonr"]
        .into_iter()
        .any(|dir| path.starts_with(home.join(dir)))
}

pub(crate) fn protected_read(path: &Path) -> bool {
    if let Some(name) = path.file_name().and_then(|name| name.to_str())
        && (name == ".env"
            || name.starts_with(".env.")
            || (name.starts_with("credentials.") && crate::secret::is_store(path)))
    {
        return true;
    }
    let Some(home) = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
    else {
        return false;
    };
    [".ssh", ".gnupg"]
        .into_iter()
        .any(|dir| path.starts_with(home.join(dir)))
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

const WRAPPERS: [&str; 15] = [
    "env", "sudo", "doas", "nice", "nohup", "exec", "command", "builtin", "time", "xargs",
    "timeout", "stdbuf", "setsid", "flock", "chronic",
];

fn pipes_to_shell(command: &str) -> bool {
    unquoted(command, &["|"])
        .into_iter()
        .skip(1)
        .any(|segment| {
            let words: Vec<&str> = segment
                .trim_start_matches('&')
                .split_whitespace()
                .map(|word| word.rsplit('/').next().unwrap_or(word))
                .collect();
            words
                .iter()
                .position(|word| {
                    !WRAPPERS.contains(word)
                        && !word.starts_with('-')
                        && !word.contains('=')
                        && !word.chars().all(|c| c.is_ascii_digit())
                })
                .is_some_and(|at| {
                    let word = words[at];
                    let bare: String = word
                        .chars()
                        .filter(|c| !matches!(c, '"' | '\'' | '\\'))
                        .collect();
                    if let Some(var) = bare.strip_prefix('$') {
                        return expands_to_shell(var);
                    }
                    let name = bare
                        .trim_end_matches(|c: char| !c.is_ascii_alphanumeric())
                        .to_ascii_lowercase();
                    shell(&name)
                        || word.starts_with('`')
                        || (interpreter(&name) && reads_stdin(&words[at + 1..]))
                })
        })
}

fn interpreter(name: &str) -> bool {
    matches!(
        name.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.'),
        "python" | "node" | "perl" | "ruby" | "php" | "deno" | "bun"
    )
}

fn reads_stdin(args: &[&str]) -> bool {
    args.contains(&"-")
        || args.iter().all(|arg| match arg.strip_prefix("--") {
            Some(long) => !["eval", "print", "command"]
                .iter()
                .any(|flag| long.starts_with(flag)),
            None => arg.starts_with('-') && !arg.contains(['c', 'e', 'E', 'm']),
        })
}

fn runs_substitution(command: &str) -> bool {
    split_segments(command).iter().any(|segment| {
        let mut words = segment.split_whitespace();
        words.next().is_some_and(|verb| {
            let name = verb.rsplit('/').next().unwrap_or(verb).to_ascii_lowercase();
            shell(&name) || interpreter(&name) || matches!(name.as_str(), "source" | ".")
        }) && words.any(|word| word.starts_with("<("))
    })
}

fn builtin_deny(command: &str) -> bool {
    let folded = collapse(command);
    wipes_root(&folded)
        || folded.contains("git push --force")
        || folded.contains("git push -f")
        || folded.contains("git push --mirror")
        || pipes_to_shell(command)
        || runs_substitution(command)
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

enum Block {
    Outside(String),
    Protected(String),
}

fn locate(word: &str, workspace: &Path) -> PathBuf {
    expand_home(word).unwrap_or_else(|| {
        let path = Path::new(word);
        if path.has_root() {
            path.to_path_buf()
        } else {
            workspace.join(path)
        }
    })
}

fn block_target(
    word: &str,
    workspace: &Path,
    outside_cwd: bool,
    roots: &[PathBuf],
) -> Option<Block> {
    if protected_write(&locate(word, workspace)) {
        return Some(Block::Protected(word.to_string()));
    }
    if outside_cwd || is_outside(word, workspace, outside_cwd, roots) {
        return Some(Block::Outside(word.to_string()));
    }
    None
}

fn write_block(command: &str, workspace: &Path, roots: &[PathBuf]) -> Option<Block> {
    let mut outside_cwd = false;
    for segment in split_segments(command) {
        let segment = if outside_cwd || !workspace.join(".git").exists() {
            segment
        } else {
            segment.replace("$(git rev-parse --show-toplevel)", ".")
        };
        let words: Vec<String> = unquoted(&segment, &[" ", "\t"])
            .into_iter()
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
        let creates = matches!(verb.as_str(), "mkdir" | "touch" | "md");
        let edits_in_place = verb == "sed"
            && words
                .iter()
                .any(|word| word.starts_with("-i") || word.starts_with("--in-place"));
        let destructive =
            DESTRUCTIVE.contains(&verb.as_str()) || deletes_via_find || edits_in_place;
        for word in &words {
            if let Some((_, target)) = word.split_once('>') {
                let target = target.trim_start_matches(['>', '&', '|']);
                let target = if target.is_empty() { continue } else { target };
                if let Some(block) = block_target(target, workspace, outside_cwd, roots) {
                    return Some(block);
                }
            }
        }
        let mut redirect_next = false;
        let mut targets: Vec<String> = Vec::new();
        for word in args.clone() {
            if redirect_next {
                redirect_next = false;
                if let Some(block) = block_target(word, workspace, outside_cwd, roots) {
                    return Some(block);
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
                if is_outside(word, workspace, outside_cwd, roots) {
                    outside_cwd = true;
                }
                break;
            }
            if creates {
                if let Some(block) = block_target(word, workspace, outside_cwd, roots) {
                    return Some(block);
                }
                continue;
            }
            if destructive {
                if in_temp(word) {
                    return Some(Block::Outside(word.clone()));
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
            if let Some(block) = block_target(word, workspace, outside_cwd, roots) {
                return Some(block);
            }
        }
    }
    None
}

const DEST_ONLY: [&str; 5] = ["cp", "mv", "ln", "install", "rsync"];

pub(crate) fn split_segments(command: &str) -> Vec<String> {
    let mut out = Vec::new();
    for segment in unquoted(command, &["&&", "||", "|", ";", "\n", "&"]) {
        let segment = unwrap_segment(&segment);
        if segment.is_empty() {
            continue;
        }
        if let Some(script) = inline_script(&segment) {
            out.extend(split_segments(&script));
        }
        out.push(segment);
    }
    out
}

fn unwrap_segment(segment: &str) -> String {
    let mut text = segment.trim().trim_start_matches(['(', '{', ' ', '\t']);
    if text.matches(')').count() > text.matches('(').count() {
        text = text.trim_end_matches([')', ' ', '\t']);
    }
    loop {
        let (word, tail) = text.split_once([' ', '\t']).unwrap_or((text, ""));
        let name = word.trim_start_matches('\\');
        let name = name.rsplit('/').next().unwrap_or(name);
        if !WRAPPERS.contains(&name) {
            return text.trim_start_matches('\\').to_string();
        }
        text = tail.trim_start();
        loop {
            let (word, rest) = text.split_once([' ', '\t']).unwrap_or((text, ""));
            if word.is_empty()
                || !(word.starts_with('-')
                    || word.contains('=')
                    || word.chars().all(|c| c.is_ascii_digit()))
            {
                break;
            }
            text = rest.trim_start();
        }
    }
}

fn inline_script(segment: &str) -> Option<String> {
    let words = unquoted(segment, &[" ", "\t"]);
    let mut words = words
        .iter()
        .map(|word| word.trim())
        .filter(|word| !word.is_empty());
    let verb = words.next()?;
    if !shell(verb) {
        return None;
    }
    words.find(|word| word.starts_with('-') && !word.starts_with("--") && word.contains('c'))?;
    let script = words.next()?;
    Some(
        ['\'', '"']
            .iter()
            .find_map(|quote| script.strip_prefix(*quote)?.strip_suffix(*quote))
            .unwrap_or(script)
            .to_string(),
    )
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
                } else if let Some(delimiter) = delimiters.iter().find(|d| {
                    rest.starts_with(*d)
                        && !(**d == "&"
                            && (current.ends_with(['>', '<']) || rest.starts_with("&>")))
                }) {
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

fn is_outside(word: &str, workspace: &Path, outside_cwd: bool, extra: &[PathBuf]) -> bool {
    if matches!(word, "/dev/null" | "/dev/stdout" | "/dev/stderr") {
        return false;
    }
    if let Some(target) = expand_home(word) {
        return !is_inside(&target, workspace, extra);
    }
    if word.starts_with('~') {
        return true;
    }
    if word.starts_with('$') {
        let name = word
            .trim_start_matches('$')
            .trim_start_matches('{')
            .trim_end_matches('}');
        let head = name.split(['/', '\\']).next().unwrap_or(name);
        return matches!(head, "HOME" | "TMPDIR" | "TMP" | "TEMP" | "TEMPDIR" | "PWD");
    }
    if Path::new(word).has_root() {
        return !is_inside(Path::new(word), workspace, extra);
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

pub(crate) fn expand_path(word: &str) -> PathBuf {
    remap_volume(expand_home(word).unwrap_or_else(|| PathBuf::from(word)))
}

fn remap_volume(path: PathBuf) -> PathBuf {
    if !cfg!(target_os = "macos") {
        return path;
    }
    let mut components = path.components();
    if components.next() != Some(Component::RootDir) {
        return path;
    }
    match components.next() {
        Some(Component::Normal(volumes)) if volumes == "Volumes" => {}
        _ => return path,
    }
    let Some(Component::Normal(asked)) = components.next() else {
        return path;
    };
    let asked = asked.to_string_lossy();
    let Ok(entries) = std::fs::read_dir("/Volumes") else {
        return path;
    };
    let names: Vec<std::ffi::OsString> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.file_name()))
        .collect();
    let equal: Vec<_> = names
        .iter()
        .filter(|name| name.to_string_lossy().eq_ignore_ascii_case(&asked))
        .cloned()
        .collect();
    let hits = if equal.len() == 1 {
        equal
    } else {
        let prefix: Vec<_> = names
            .iter()
            .filter(|name| {
                name.to_string_lossy()
                    .to_ascii_lowercase()
                    .starts_with(&asked.to_ascii_lowercase())
            })
            .cloned()
            .collect();
        if prefix.len() == 1 {
            prefix
        } else {
            return path;
        }
    };
    if hits[0].to_string_lossy() == asked {
        return path;
    }
    let mut out = PathBuf::from("/Volumes");
    out.push(&hits[0]);
    for rest in components {
        out.push(rest);
    }
    out
}

fn expand_home(word: &str) -> Option<PathBuf> {
    let rest = word
        .strip_prefix("~/")
        .or_else(|| word.strip_prefix("$HOME/"))?;
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(PathBuf::from(home).join(rest))
}

fn resolved(path: &Path) -> PathBuf {
    if let Ok(canonical) = path.canonicalize() {
        return canonical;
    }
    path.parent()
        .and_then(|parent| parent.canonicalize().ok())
        .map_or_else(
            || path.to_path_buf(),
            |parent| match path.file_name() {
                Some(name) => parent.join(name),
                None => parent,
            },
        )
}

fn is_inside(target: &Path, workspace: &Path, extra: &[PathBuf]) -> bool {
    let temp = std::env::temp_dir();
    let target = resolved(target);
    let roots = std::iter::once(workspace).chain(extra.iter().map(PathBuf::as_path));
    roots
        .flat_map(|root| [root.to_path_buf(), resolved(root)])
        .chain(
            [temp.clone(), temp.canonicalize().unwrap_or_default()]
                .into_iter()
                .filter(|root| !root.as_os_str().is_empty()),
        )
        .any(|root| target.starts_with(&root))
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
