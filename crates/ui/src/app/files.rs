use super::*;

pub(crate) fn save(path: Option<&Path>, prefs: &Prefs) -> Result<(), String> {
    let path = path.ok_or_else(|| "settings not saved: home directory unavailable".to_string())?;
    prefs::save_in(path, prefs).map_err(|error| format!("settings not saved: {error}"))
}

pub(crate) fn file_url(text: &str) -> Option<PathBuf> {
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

pub(crate) fn percent_decode(text: &str) -> String {
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

pub(crate) fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub(crate) fn read_git_branch(workspace: &Path) -> Option<String> {
    let dot = workspace.join(".git");
    let dir = match std::fs::read_to_string(&dot) {
        Ok(link) => workspace.join(link.trim().strip_prefix("gitdir:")?.trim()),
        Err(_) => dot,
    };
    let head = std::fs::read_to_string(dir.join("HEAD")).ok()?;
    let head = head.trim();
    head.strip_prefix("ref: refs/heads/")
        .map(str::to_string)
        .or_else(|| (!head.is_empty()).then(|| head.chars().take(7).collect()))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PaneRow {
    pub(crate) path: String,
    pub(crate) name: String,
    pub(crate) depth: usize,
    pub(crate) dir: bool,
    pub(crate) open: bool,
    pub(crate) status: String,
    pub(crate) badge: Option<(String, sigil::prelude::Tone)>,
    pub(crate) note: Option<String>,
    pub(crate) detail: Vec<String>,
}

impl PaneRow {
    pub(crate) fn flat(path: String, name: String, open: bool) -> Self {
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

    pub(crate) fn status(mut self, status: String) -> Self {
        self.status = status;
        self
    }

    pub(crate) fn badge(mut self, label: impl Into<String>, tone: sigil::prelude::Tone) -> Self {
        self.badge = Some((label.into(), tone));
        self
    }

    pub(crate) fn note(mut self, note: Option<String>) -> Self {
        self.note = note;
        self
    }

    pub(crate) fn detail(mut self, lines: impl IntoIterator<Item = String>) -> Self {
        if self.open {
            self.detail = lines.into_iter().collect();
        }
        self
    }
}

pub(crate) fn file_rows(files: &[String], expanded: &[String]) -> Vec<PaneRow> {
    let expanded: std::collections::HashSet<&str> = expanded.iter().map(String::as_str).collect();
    let mut rows: Vec<PaneRow> = Vec::new();
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
                rows.push(PaneRow {
                    depth,
                    dir: !last,
                    ..PaneRow::flat(path.clone(), (*part).to_string(), !shut)
                });
            }
            hidden |= shut;
            prefix = path;
        }
    }
    rows
}

pub(crate) type Confirm<K> = Signal<Option<(K, std::time::Instant)>>;

pub(crate) fn confirm<K: Clone + PartialEq + 'static>(slot: &Confirm<K>, key: K) -> bool {
    let repeat = live(slot.peek(), &key);
    slot.set(if repeat {
        None
    } else {
        Some((key, std::time::Instant::now()))
    });
    repeat
}

pub(crate) fn armed<K: Clone + PartialEq + 'static>(slot: &Confirm<K>, key: &K) -> bool {
    live(slot.get(), key)
}

pub(super) fn expire<K: Clone + PartialEq + 'static>(slot: Confirm<K>) {
    sigil::prelude::effect(move || {
        if let Some((_, at)) = slot.get() {
            let left = summonr_agent::limits::limits()
                .repeat_window()
                .saturating_sub(at.elapsed());
            sigil::reactive::timeout(left, move || {
                if slot.peek().is_some_and(|(_, armed)| armed == at) {
                    slot.set(None);
                }
            });
        }
    });
}

fn live<K: PartialEq>(entry: Option<(K, std::time::Instant)>, key: &K) -> bool {
    let window = summonr_agent::limits::limits().repeat_window();
    entry.is_some_and(|(armed, at)| armed == *key && at.elapsed() <= window)
}
