use super::shell::WRAPPERS;
use super::*;

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

pub(super) fn unwrap_segment(segment: &str) -> String {
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

pub(super) fn inline_script(segment: &str) -> Option<String> {
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
    let script = words.find(|word| *word != "--")?;
    Some(
        ['\'', '"']
            .iter()
            .find_map(|quote| script.strip_prefix(*quote)?.strip_suffix(*quote))
            .unwrap_or(script)
            .to_string(),
    )
}

pub(super) fn unquoted(command: &str, delimiters: &[&str]) -> Vec<String> {
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

pub(super) fn in_temp(word: &str) -> bool {
    let temp = std::env::temp_dir();
    let target = Path::new(word.trim_end_matches('*'));
    target.has_root()
        && [temp.clone(), temp.canonicalize().unwrap_or_default()]
            .iter()
            .any(|root| !root.as_os_str().is_empty() && target.starts_with(root))
}

pub(super) fn is_outside(
    word: &str,
    workspace: &Path,
    outside_cwd: bool,
    extra: &[PathBuf],
) -> bool {
    if matches!(word, "/dev/null" | "/dev/stdout" | "/dev/stderr") {
        return false;
    }
    if let Some(target) = crate::config::expand_tilde(word) {
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
    remap_volume(crate::config::expand_tilde(word).unwrap_or_else(|| PathBuf::from(word)))
}

pub(super) fn remap_volume(path: PathBuf) -> PathBuf {
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

pub(super) fn resolved(path: &Path) -> PathBuf {
    if let Ok(canonical) = dunce::canonicalize(path) {
        return canonical;
    }
    path.parent()
        .and_then(|parent| dunce::canonicalize(parent).ok())
        .map_or_else(
            || path.to_path_buf(),
            |parent| match path.file_name() {
                Some(name) => parent.join(name),
                None => parent,
            },
        )
}

pub(super) fn is_inside(target: &Path, workspace: &Path, extra: &[PathBuf]) -> bool {
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

pub(super) fn wipes_root(folded: &str) -> bool {
    ["rm -rf /", "rm -fr /", "rm -r /"].iter().any(|prefix| {
        folded.match_indices(prefix).any(|(index, _)| {
            matches!(
                folded[index + prefix.len()..].chars().next(),
                None | Some(' ') | Some('*')
            )
        })
    })
}

pub(super) fn collapse(command: &str) -> String {
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

pub(super) fn matches_allow(command: &str, pattern: &str) -> bool {
    let command = collapse(command);
    let pattern = collapse(pattern);
    if pattern.is_empty() {
        return false;
    }
    command == pattern || command.starts_with(&format!("{pattern} "))
}

pub(super) fn matches_deny(command: &str, pattern: &str) -> bool {
    let command = collapse(command);
    let pattern = collapse(pattern);
    if pattern.is_empty() {
        return false;
    }
    command == pattern || command.starts_with(&format!("{pattern} ")) || command.contains(&pattern)
}

pub(super) fn path_matches(path: &str, glob: &str) -> bool {
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
