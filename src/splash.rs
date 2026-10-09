use std::io::IsTerminal;
use std::path::Path;

pub struct Splash<'a> {
    pub version: &'a str,
    pub model: &'a str,
    pub relay: Option<&'a str>,
    pub session: &'a str,
    pub resumed: bool,
    pub history: usize,
    pub workspace: &'a Path,
    pub strict: bool,
    pub trust: bool,
}

struct Palette {
    accent: &'static str,
    dim: &'static str,
    bold: &'static str,
    reset: &'static str,
}

const ANSI: Palette = Palette {
    accent: "\x1b[1;35m",
    dim: "\x1b[2m",
    bold: "\x1b[1m",
    reset: "\x1b[0m",
};
const PLAIN: Palette = Palette {
    accent: "",
    dim: "",
    bold: "",
    reset: "",
};

const LOGO: [&str; 6] = [
    "███████╗██╗   ██╗███╗   ███╗███╗   ███╗ ██████╗ ███╗   ██╗██████╗ ",
    "██╔════╝██║   ██║████╗ ████║████╗ ████║██╔═══██╗████╗  ██║██╔══██╗",
    "███████╗██║   ██║██╔████╔██║██╔████╔██║██║   ██║██╔██╗ ██║██████╔╝",
    "╚════██║██║   ██║██║╚██╔╝██║██║╚██╔╝██║██║   ██║██║╚██╗██║██╔══██╗",
    "███████║╚██████╔╝██║ ╚═╝ ██║██║ ╚═╝ ██║╚██████╔╝██║ ╚████║██║  ██║",
    "╚══════╝ ╚═════╝ ╚═╝     ╚═╝╚═╝     ╚═╝ ╚═════╝ ╚═╝  ╚═══╝╚═╝  ╚═╝",
];
const LOGO_WIDTH: usize = 66;
const LABEL_WIDTH: usize = 8;

pub(crate) fn logo() -> &'static [&'static str; 6] {
    &LOGO
}

#[must_use]
pub(crate) fn logo_width() -> usize {
    LOGO_WIDTH
}

impl Splash<'_> {
    pub fn interactive(&self, full: bool) {
        if !full || !std::io::stderr().is_terminal() {
            self.line();
            return;
        }
        let palette = if colored() { &ANSI } else { &PLAIN };
        eprintln!();
        eprintln!("{}", self.render(palette));
    }

    pub fn line(&self) {
        match self.relay {
            Some(relay) => eprintln!(
                "summonr {} · {} @ {} · session {}",
                self.version,
                self.model,
                relay,
                crate::session::short(self.session)
            ),
            None => eprintln!(
                "summonr {} · {} · session {}",
                self.version,
                self.model,
                crate::session::short(self.session)
            ),
        }
    }

    #[must_use]
    pub(crate) fn facts(&self) -> Vec<(&'static str, String)> {
        let mut rows = vec![("summonr", self.version.to_string())];
        let model = match self.relay {
            Some(relay) => format!("{} · {relay}", self.model),
            None => self.model.to_string(),
        };
        rows.push(("model", model));
        let state = if self.resumed { "resumed" } else { "new" };
        rows.push(("session", format!("{} · {state}", self.session)));
        if self.resumed && self.history > 0 {
            rows.push(("history", format!("{} messages", self.history)));
        }
        rows.push(("workdir", crate::config::tilde(self.workspace)));
        if self.strict {
            rows.push(("policy", "strict".to_string()));
        }
        if self.trust {
            rows.push(("context", "AGENTS.md loaded".to_string()));
        }
        rows
    }

    fn render(&self, palette: &Palette) -> String {
        let info = self.info_lines(palette);
        let mut lines = Vec::new();
        for row in LOGO {
            lines.push(format!("{}{row}{}", palette.accent, palette.reset));
        }
        lines.push(String::new());
        lines.extend(info.iter().map(|row| format!("  {row}")));
        lines.push(String::new());
        lines.push(format!(
            "{}✦ /help · @file · /exit{}",
            palette.dim, palette.reset
        ));
        lines.join("\n")
    }

    fn info_lines(&self, palette: &Palette) -> Vec<String> {
        let mut rows = Vec::new();
        for (index, (label, value)) in self.facts().into_iter().enumerate() {
            if index == 0 {
                rows.push(format!("{}summonr {value}{}", palette.bold, palette.reset));
            } else {
                rows.push(row(palette, label, &value));
            }
        }
        rows
    }
}

fn colored() -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    !matches!(std::env::var("TERM"), Ok(term) if term == "dumb")
}

fn row(palette: &Palette, label: &str, value: &str) -> String {
    format!(
        "{}{label:<LABEL_WIDTH$}{} {value}",
        palette.dim, palette.reset
    )
}

pub fn host_of(url: Option<&str>) -> Option<&str> {
    let url = url?;
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let host = rest.split('/').next().unwrap_or("");
    (!host.is_empty()).then_some(host)
}
