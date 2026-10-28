use super::*;

pub(in crate::tui::sigil_view) fn save(path: Option<&Path>, prefs: &Prefs) -> Result<(), String> {
    let path = path.ok_or_else(|| "settings not saved: home directory unavailable".to_string())?;
    prefs::save_in(path, prefs).map_err(|error| format!("settings not saved: {error}"))
}

pub(in crate::tui::sigil_view) fn file_url(text: &str) -> Option<PathBuf> {
    let rest = text.strip_prefix("file:")?;
    let decoded = percent_decode(rest);
    let (host, path) = if let Some(rest) = decoded.strip_prefix("//") {
        match rest.find('/') {
            Some(split) => (&rest[..split], &rest[split..]),
            None => (rest, ""),
        }
    } else {
        ("", decoded.as_str())
    };
    if path.is_empty() {
        return None;
    }
    if !host.is_empty() && !host.eq_ignore_ascii_case("localhost") {
        return None;
    }
    let path = match path.as_bytes() {
        [b'/', drive, b':', ..] if drive.is_ascii_alphabetic() => &path[1..],
        _ => path,
    };
    Some(PathBuf::from(path))
}

pub(in crate::tui::sigil_view) fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && let Some(high) = bytes.get(index + 1).and_then(|byte| hex_digit(*byte))
            && let Some(low) = bytes.get(index + 2).and_then(|byte| hex_digit(*byte))
        {
            out.push(high * 16 + low);
            index += 3;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub(in crate::tui::sigil_view) fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub(in crate::tui::sigil_view) fn read_git_branch(workspace: &Path) -> Option<String> {
    let head = std::fs::read_to_string(workspace.join(".git/HEAD")).ok()?;
    let head = head.trim();
    head.strip_prefix("ref: refs/heads/")
        .map(str::to_string)
        .or_else(|| (!head.is_empty()).then(|| head.chars().take(7).collect()))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::tui::sigil_view) struct FileRow {
    pub(in crate::tui::sigil_view) path: String,
    pub(in crate::tui::sigil_view) name: String,
    pub(in crate::tui::sigil_view) depth: usize,
    pub(in crate::tui::sigil_view) dir: bool,
    pub(in crate::tui::sigil_view) open: bool,
    pub(in crate::tui::sigil_view) status: String,
    pub(in crate::tui::sigil_view) badge: Option<(String, sigil::prelude::Tone)>,
    pub(in crate::tui::sigil_view) note: Option<String>,
    pub(in crate::tui::sigil_view) detail: Vec<String>,
}

impl FileRow {
    pub(in crate::tui::sigil_view) fn flat(path: String, name: String, open: bool) -> Self {
        Self {
            path,
            name,
            depth: 0,
            dir: false,
            open,
            status: String::new(),
            badge: None,
            note: None,
            detail: Vec::new(),
        }
    }

    pub(in crate::tui::sigil_view) fn status(mut self, status: String) -> Self {
        self.status = status;
        self
    }

    pub(in crate::tui::sigil_view) fn badge(
        mut self,
        label: impl Into<String>,
        tone: sigil::prelude::Tone,
    ) -> Self {
        self.badge = Some((label.into(), tone));
        self
    }

    pub(in crate::tui::sigil_view) fn note(mut self, note: Option<String>) -> Self {
        self.note = note;
        self
    }

    pub(in crate::tui::sigil_view) fn detail(
        mut self,
        lines: impl IntoIterator<Item = String>,
    ) -> Self {
        if self.open {
            self.detail = lines.into_iter().collect();
        }
        self
    }
}

pub(in crate::tui::sigil_view) fn file_rows(files: &[String], expanded: &[String]) -> Vec<FileRow> {
    let expanded: std::collections::HashSet<&str> = expanded.iter().map(String::as_str).collect();
    let mut rows: Vec<FileRow> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut sorted: Vec<&String> = files.iter().collect();
    sorted.sort();
    for file in sorted {
        let parts: Vec<&str> = file.split('/').filter(|part| !part.is_empty()).collect();
        let mut prefix = String::new();
        let mut hidden = false;
        for (depth, part) in parts.iter().enumerate() {
            let last = depth + 1 == parts.len();
            let path = if prefix.is_empty() {
                (*part).to_string()
            } else {
                format!("{prefix}/{part}")
            };
            let shut = !last && !expanded.contains(path.as_str());
            if !hidden && seen.insert(path.clone()) {
                rows.push(FileRow {
                    depth,
                    dir: !last,
                    ..FileRow::flat(path.clone(), (*part).to_string(), !shut)
                });
            }
            hidden |= shut;
            prefix = path;
        }
    }
    rows
}

pub(in crate::tui::sigil_view) fn armed(
    slot: &Cell<Option<std::time::Instant>>,
    window: std::time::Duration,
) -> bool {
    let now = std::time::Instant::now();
    let repeat = slot
        .get()
        .is_some_and(|last| now.duration_since(last) <= window);
    slot.set(if repeat { None } else { Some(now) });
    repeat
}
