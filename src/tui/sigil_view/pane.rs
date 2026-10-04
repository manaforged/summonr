#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Pane {
    Chat,
    Ledger,
    Files,
    Diff,
    Worktrees,
    Jobs,
    Mcp,
    Peers,
    View,
}

impl Pane {
    pub(super) const SECONDARY: [Pane; 7] = [
        Pane::Ledger,
        Pane::Files,
        Pane::Diff,
        Pane::Worktrees,
        Pane::Jobs,
        Pane::Mcp,
        Pane::Peers,
    ];

    pub(super) fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "chat" => Some(Self::Chat),
            "ledger" | "plan" => Some(Self::Ledger),
            "files" | "tree" | "repo" => Some(Self::Files),
            "diff" | "changes" => Some(Self::Diff),
            "worktrees" | "trees" => Some(Self::Worktrees),
            "jobs" | "job" | "background" => Some(Self::Jobs),
            "mcp" | "servers" => Some(Self::Mcp),
            "peers" | "agents" | "live" => Some(Self::Peers),
            "view" | "file" => Some(Self::View),
            _ => None,
        }
    }

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Ledger => "ledger",
            Self::Files => "files",
            Self::Diff => "diff",
            Self::Worktrees => "worktrees",
            Self::Jobs => "jobs",
            Self::Mcp => "mcp",
            Self::Peers => "peers",
            Self::View => "view",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Layout {
    pub(super) secondary: Option<Pane>,
    pub(super) share: u16,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            secondary: None,
            share: 40,
        }
    }
}

impl Layout {
    pub(super) const MIN_CHAT: u16 = 48;
    pub(super) const MIN_SIDE: u16 = 20;
    pub(super) const MIN_COLS: u16 = Self::MIN_CHAT + Self::MIN_SIDE + 1;

    pub(super) fn parse(text: &str) -> Option<Self> {
        if let Some(preset) = Self::preset(text) {
            return Some(preset);
        }
        let mut parts = text.split('|').map(str::trim);
        if Pane::parse(parts.next()?)? != Pane::Chat {
            return None;
        }
        let Some(side) = parts.next() else {
            return Some(Self::default());
        };
        let mut words = side.split_whitespace();
        let pane = Pane::parse(words.next()?)?;
        if pane == Pane::Chat || parts.next().is_some() {
            return None;
        }
        let share = words
            .next()
            .and_then(|share| share.trim_end_matches('%').parse::<u16>().ok())
            .unwrap_or(40)
            .clamp(20, 60);
        Some(Self {
            secondary: Some(pane),
            share,
        })
    }

    pub(super) fn preset(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "focus" => Some(Self::default()),
            "run" => Some(Self {
                secondary: Some(Pane::Ledger),
                share: 20,
            }),
            "files" | "explore" => Some(Self {
                secondary: Some(Pane::Files),
                share: 30,
            }),
            "review" | "diff" => Some(Self {
                secondary: Some(Pane::Diff),
                share: 45,
            }),
            _ => None,
        }
    }

    pub(super) fn text(self) -> String {
        match self.secondary {
            None => "chat".to_string(),
            Some(pane) => format!("chat | {} {}%", pane.name(), self.share),
        }
    }

    pub(super) fn split(self, cols: u16) -> (u16, u16) {
        let Some(_) = self.secondary else {
            return (cols, 0);
        };
        if cols < Self::MIN_COLS {
            return (cols, 0);
        }
        let side = (u32::from(cols) * u32::from(self.share) / 100) as u16;
        let side = side.clamp(Self::MIN_SIDE, cols - Self::MIN_CHAT - 1);
        (cols - side - 1, side)
    }

    pub(super) fn nudge(self, delta: i16) -> Self {
        Self {
            share: (i32::from(self.share) + i32::from(delta)).clamp(20, 60) as u16,
            ..self
        }
    }
}
