//! Startup banner. Full splash on an interactive terminal, one line otherwise.
//!
//! Everything here is deterministic: same input, same bytes. Color is
//! applied only when stderr is a terminal, `NO_COLOR` is unset, and TERM
//! is not `dumb`. `--no-splash` drops to the compact line form.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};

/// Everything the splash knows how to draw.
pub struct Splash<'a> {
    pub version: &'a str,
    pub model: &'a str,
    /// Relay host for OpenAI-compatible endpoints, when there is one.
    pub relay: Option<&'a str>,
    pub session: &'a str,
    pub resumed: bool,
    pub history: usize,
    pub workspace: &'a Path,
    pub plan: bool,
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
    "███████╗██╗   ██╗███╗   ███╗███╗   ███╗ ██████╗ ███╗   ██╗███████╗██████╗ ",
    "██╔════╝██║   ██║████╗ ████║████╗ ████║██╔═══██╗████╗  ██║██╔════╝██╔══██╗",
    "███████╗██║   ██║██╔████╔██║██╔████╔██║██║   ██║██╔██╗ ██║█████╗  ██████╔╝",
    "╚════██║██║   ██║██║╚██╔╝██║██║╚██╔╝██║██║   ██║██║╚██╗██║██╔══╝  ██╔══██╗",
    "███████║╚██████╔╝██║ ╚═╝ ██║██║ ╚═╝ ██║╚██████╔╝██║ ╚████║███████╗██║  ██║",
    "╚══════╝ ╚═════╝ ╚═╝     ╚═╝╚═╝     ╚═╝ ╚═════╝ ╚═╝  ╚═══╝╚══════╝╚═╝  ╚═╝",
];
const LOGO_WIDTH: usize = 74;
const LABEL_WIDTH: usize = 8;

/// The block logo, for the splash line and the TUI cover.
pub fn logo() -> &'static [&'static str; 6] {
    &LOGO
}

/// Rendered width of [`logo`].
#[must_use]
pub fn logo_width() -> usize {
    LOGO_WIDTH
}

/// Home-abbreviated display form of a path.
#[must_use]
pub fn short_path(path: &Path) -> String {
    display_path(path)
}

/// Print the splash for an interactive session. `full` false (or a piped
/// stderr, or `--no-splash`) falls back to the compact status line.
pub fn interactive(splash: &Splash, full: bool) {
    if !full || !std::io::stderr().is_terminal() {
        line(splash);
        return;
    }
    let palette = if colored() { &ANSI } else { &PLAIN };
    eprintln!();
    eprintln!("{}", render(splash, palette));
}

/// Compact one-line status, for print mode and non-terminal stderr.
pub fn line(splash: &Splash) {
    match splash.relay {
        Some(relay) => eprintln!(
            "summoner {} · {} @ {} · session {}",
            splash.version, splash.model, relay, splash.session
        ),
        None => eprintln!(
            "summoner {} · {} · session {}",
            splash.version, splash.model, splash.session
        ),
    }
}

fn colored() -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    !matches!(std::env::var("TERM"), Ok(term) if term == "dumb")
}

/// Raw label / value rows shown on the splash, unstyled, for other fronts.
#[must_use]
pub fn facts(splash: &Splash) -> Vec<(&'static str, String)> {
    let mut rows = vec![("summoner", splash.version.to_string())];
    let model = match splash.relay {
        Some(relay) => format!("{} · {relay}", splash.model),
        None => splash.model.to_string(),
    };
    rows.push(("model", model));
    let state = if splash.resumed { "resumed" } else { "new" };
    rows.push(("session", format!("{} · {state}", splash.session)));
    if splash.resumed && splash.history > 0 {
        rows.push(("history", format!("{} messages", splash.history)));
    }
    rows.push(("workdir", display_path(splash.workspace)));
    if splash.plan {
        rows.push(("mode", "plan · read-only".to_string()));
    }
    if splash.strict {
        rows.push(("policy", "strict".to_string()));
    }
    if splash.trust {
        rows.push(("context", "AGENTS.md loaded".to_string()));
    }
    rows
}

fn render(splash: &Splash, palette: &Palette) -> String {
    let info = info_lines(splash, palette);
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

fn info_lines(splash: &Splash, palette: &Palette) -> Vec<String> {
    let mut rows = Vec::new();
    for (index, (label, value)) in facts(splash).into_iter().enumerate() {
        if index == 0 {
            rows.push(format!("{}summoner {value}{}", palette.bold, palette.reset));
        } else {
            rows.push(row(palette, label, &value));
        }
    }
    rows
}

fn row(palette: &Palette, label: &str, value: &str) -> String {
    format!(
        "{}{label:<LABEL_WIDTH$}{} {value}",
        palette.dim, palette.reset
    )
}

/// Host part of a base URL: `https://open.bigmodel.cn/api/…` → `open.bigmodel.cn`.
pub fn host_of(url: Option<&str>) -> Option<&str> {
    let url = url?;
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let host = rest.split('/').next().unwrap_or("");
    (!host.is_empty()).then_some(host)
}

/// `~/src/summoner` style path when it sits under `home`.
fn abbreviate(path: &Path, home: Option<&Path>) -> String {
    if let Some(home) = home
        && let Ok(rest) = path.strip_prefix(home)
    {
        let rest = rest.to_string_lossy();
        if rest.is_empty() {
            return "~".to_string();
        }
        return format!("~/{rest}");
    }
    path.to_string_lossy().into_owned()
}

fn display_path(path: &Path) -> String {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    abbreviate(path, home.as_deref())
}

#[cfg(test)]
#[path = "splash_tests.rs"]
mod tests;
