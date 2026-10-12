use crate::tools::TodoStatus;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::tui::sigil_view) enum Mark {
    Done,
    Active,
    Pending,
    Blocked,
    Plain,
}

impl Mark {
    pub(in crate::tui::sigil_view) const fn glyph(self) -> char {
        match self {
            Self::Done => '✓',
            Self::Active => '▸',
            Self::Pending => '◦',
            Self::Blocked => '!',
            Self::Plain => ' ',
        }
    }

    pub(in crate::tui::sigil_view) const fn settled(self) -> bool {
        matches!(self, Self::Done | Self::Plain)
    }
}

impl From<TodoStatus> for Mark {
    fn from(status: TodoStatus) -> Self {
        match status {
            TodoStatus::Done => Self::Done,
            TodoStatus::Active => Self::Active,
            TodoStatus::Pending => Self::Pending,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::tui::sigil_view) struct LedgerRow {
    pub(in crate::tui::sigil_view) indent: usize,
    pub(in crate::tui::sigil_view) mark: Mark,
    pub(in crate::tui::sigil_view) text: String,
}

impl LedgerRow {
    pub(in crate::tui::sigil_view) fn parse(line: &str) -> Option<Self> {
        let body = line.trim_start();
        let indent = line.len() - body.len();
        for (prefix, mark) in [
            ("[x] ", Mark::Done),
            ("[/] ", Mark::Active),
            ("[ ] ", Mark::Pending),
            ("[!] ", Mark::Blocked),
        ] {
            if let Some(rest) = body.strip_prefix(prefix) {
                return Some(Self {
                    indent,
                    mark,
                    text: rest.to_string(),
                });
            }
        }
        (body.chars().next().is_some_and(|c| c.is_ascii_digit()) && body.contains(". ")).then(
            || Self {
                indent,
                mark: Mark::Plain,
                text: body.to_string(),
            },
        )
    }

    pub(in crate::tui::sigil_view) fn todo(
        todo: &crate::tools::Todo,
        indent: usize,
        tag: Option<&str>,
    ) -> Self {
        Self {
            indent,
            mark: todo.status.into(),
            text: tag.map_or_else(|| todo.text.clone(), |tag| format!("{tag} · {}", todo.text)),
        }
    }

    pub(in crate::tui::sigil_view) fn shown(&self) -> String {
        let pad = " ".repeat(self.indent);
        match self.mark {
            Mark::Plain => format!("{pad}{}", self.text),
            mark => format!("{pad}{} {}", mark.glyph(), self.text),
        }
    }
}
