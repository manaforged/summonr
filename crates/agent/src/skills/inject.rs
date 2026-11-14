use std::path::Path;

const INLINE: &str = "!`";
const TICK: char = '`';
const FENCE: &str = "```!";
const CLOSE: &str = "```";

/// One part of a skill or command body: text as written, or a `!` block whose command runs before the model sees the body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Piece<T> {
    Text(T),
    Shell { pattern: T, command: T },
}

/// Where a skill or command file lives: outside the workspace, or in the workspace's own `.summonr` or `.claude` folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Scope {
    User,
    Project,
}

impl Scope {
    /// The scope of the file or folder at `path` for `workspace`.
    pub(crate) fn of(path: &Path, workspace: &Path) -> Self {
        let project = [
            summonr_core::dirs::workspace(workspace),
            summonr_core::dirs::claude(workspace),
        ]
        .iter()
        .any(|dir| path.starts_with(dir));
        if project { Self::Project } else { Self::User }
    }
}

/// A rendered skill or command body, in pieces, with the scope of the file that defines it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Body {
    pub(crate) pieces: Vec<Piece<String>>,
    pub(crate) scope: Scope,
}

impl Body {
    /// `template` split once into text and `!` blocks; text gets `arguments` and `content`, commands get `content` only.
    pub(crate) fn render(
        template: &str,
        arguments: Option<(&str, &[String])>,
        scope: Scope,
        content: impl Fn(&str) -> String,
    ) -> Self {
        let parts = pieces(template);
        let texts: Vec<&str> = parts
            .iter()
            .filter_map(|part| match part {
                Piece::Text(text) => Some(*text),
                Piece::Shell { .. } => None,
            })
            .collect();
        let mut filled = match arguments {
            Some((arguments, names)) => super::fill::fill_parts(&texts, arguments, names),
            None => texts.iter().map(|text| (*text).to_owned()).collect(),
        }
        .into_iter();
        let pieces = parts
            .into_iter()
            .map(|part| match part {
                Piece::Text(_) => Piece::Text(content(&filled.next().unwrap_or_default())),
                Piece::Shell { pattern, command } => Piece::Shell {
                    pattern: pattern.to_owned(),
                    command: content(command),
                },
            })
            .collect();
        Self { pieces, scope }
    }

    /// The body as written, each block shown as its pattern.
    #[cfg(test)]
    pub(crate) fn written(&self) -> String {
        self.pieces
            .iter()
            .map(|piece| match piece {
                Piece::Text(text) => text.as_str(),
                Piece::Shell { pattern, .. } => pattern.as_str(),
            })
            .collect()
    }
}

fn pieces(text: &str) -> Vec<Piece<&str>> {
    let mut out = Vec::new();
    let (mut start, mut at) = (0, 0);
    while let Some(ch) = text[at..].chars().next() {
        match block(text, at) {
            Some((length, command)) => {
                out.push(Piece::Text(&text[start..at]));
                out.push(Piece::Shell {
                    pattern: &text[at..at + length],
                    command,
                });
                at += length;
                start = at;
            }
            None => at += ch.len_utf8(),
        }
    }
    out.push(Piece::Text(&text[start..]));
    out
}

fn block(text: &str, at: usize) -> Option<(usize, &str)> {
    let rest = &text[at..];
    let line_start = text[..at].is_empty() || text[..at].ends_with('\n');
    if line_start && rest.starts_with(FENCE) {
        return fenced(rest);
    }
    let spaced = text[..at].is_empty() || text[..at].ends_with(char::is_whitespace);
    if spaced && rest.starts_with(INLINE) {
        return inline(rest);
    }
    None
}

fn inline(rest: &str) -> Option<(usize, &str)> {
    let inner = &rest[INLINE.len()..];
    let close = inner.find([TICK, '\n'])?;
    let command = &inner[..close];
    (inner[close..].starts_with(TICK) && !command.trim().is_empty())
        .then_some((INLINE.len() + close + TICK.len_utf8(), command))
}

fn fenced(rest: &str) -> Option<(usize, &str)> {
    let open = rest.find('\n')?;
    if !rest[FENCE.len()..open].trim().is_empty() {
        return None;
    }
    let body = open + 1;
    let mut line = body;
    loop {
        let end = rest[line..].find('\n').map_or(rest.len(), |at| line + at);
        if rest[line..end].trim_end() == CLOSE {
            let command = rest[body..line].trim_end();
            return (!command.trim().is_empty()).then_some((end, command));
        }
        if end == rest.len() {
            return None;
        }
        line = end + 1;
    }
}
