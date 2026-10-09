use std::path::Path;

use crate::config;
use crate::error::Result;

mod background;
mod diff;
mod edit;
mod hotkeys;
mod notify;
pub(crate) use background::Background;
pub(crate) use diff::DiffLayout;
pub(crate) use edit::banned_edit;
pub use hotkeys::Hotkey;
pub(crate) use hotkeys::{EditorKey, Hotkeys, available};
pub(crate) use notify::Notify;

pub(crate) const AUTO_THEME: &str = "auto";
pub(crate) const THEMES: [&str; 4] = ["space", "nord", "dawn", AUTO_THEME];
const RETIRED_THEMES: [(&str, &str); 4] = [
    ("light", "dawn"),
    ("fantasy", "space"),
    ("gruvbox", "space"),
    ("solarized", "space"),
];

pub(crate) fn theme(name: &str) -> &'static str {
    let name = name.trim();
    THEMES
        .into_iter()
        .find(|known| *known == name)
        .or_else(|| {
            RETIRED_THEMES
                .into_iter()
                .find(|(retired, _)| *retired == name)
                .map(|(_, now)| now)
        })
        .unwrap_or(THEMES[0])
}

pub(crate) fn known_theme(name: &str) -> bool {
    let name = name.trim();
    THEMES.contains(&name) || RETIRED_THEMES.iter().any(|(retired, _)| *retired == name)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ToolOutput {
    Compact,
    Full,
    Hidden,
}

impl ToolOutput {
    pub(crate) fn text(self, output: &str) -> &str {
        if self == Self::Full {
            return output;
        }
        let line = output
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("");
        let end = line
            .char_indices()
            .nth(120)
            .map_or(line.len(), |(index, _)| index);
        &line[..end]
    }

    #[must_use]
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Compact => "compact",
            Self::Full => "full",
            Self::Hidden => "hidden",
        }
    }

    #[must_use]
    pub(crate) fn toggle(self) -> Self {
        match self {
            Self::Compact => Self::Full,
            Self::Full => Self::Hidden,
            Self::Hidden => Self::Compact,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Visibility {
    Off,
    Collapsed,
    Expanded,
}

impl Visibility {
    #[must_use]
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Collapsed => "collapsed",
            Self::Expanded => "expanded",
        }
    }

    #[must_use]
    pub(crate) fn toggle(self) -> Self {
        match self {
            Self::Off => Self::Collapsed,
            Self::Collapsed => Self::Expanded,
            Self::Expanded => Self::Off,
        }
    }

    #[must_use]
    pub(crate) fn parse(value: &str, fallback: Self) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "off" | "false" | "hidden" => Self::Off,
            "collapsed" => Self::Collapsed,
            "expanded" | "on" | "true" | "shown" => Self::Expanded,
            _ => fallback,
        }
    }
}

pub(crate) const FPS_DEFAULT: u32 = 25;
pub(crate) const FPS_MIN: u32 = 1;
pub(crate) const FPS_MAX: u32 = 240;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prefs {
    pub(crate) theme: String,
    pub(crate) background: Background,
    pub(crate) notify: Notify,
    pub splash: bool,
    pub(crate) animations: bool,
    pub(crate) fps: u32,
    pub(crate) reasoning: Visibility,
    pub(crate) receipts: Visibility,
    pub(crate) tools: ToolOutput,
    pub(crate) diff_layout: DiffLayout,
    pub(crate) mail: bool,
    pub(crate) ban_comments: bool,
    pub(crate) ban_tests: bool,
    pub(crate) plan_mode: bool,
    pub(crate) layout: String,
    pub(crate) hotkeys: Hotkeys,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            theme: "space".to_string(),
            background: Background::Theme,
            notify: Notify::Turn,
            splash: true,
            animations: true,
            fps: FPS_DEFAULT,
            reasoning: Visibility::Collapsed,
            receipts: Visibility::Off,
            tools: ToolOutput::Compact,
            diff_layout: DiffLayout::Split,
            mail: true,
            ban_comments: false,
            ban_tests: false,
            plan_mode: true,
            hotkeys: Hotkeys::default(),
            layout: "chat".to_string(),
        }
    }
}

#[cfg(test)]
pub(crate) fn load_from(path: &Path) -> Result<Prefs> {
    from_file(&config::parse(path)?)
}

fn from_file(file: &config::File) -> Result<Prefs> {
    let base = Prefs::default();
    Ok(Prefs {
        hotkeys: Hotkeys::configured(&file.hotkeys)?,
        theme: file.theme.clone().unwrap_or(base.theme),
        background: file.background.unwrap_or(base.background),
        notify: file.notify.unwrap_or(base.notify),
        splash: file.splash.unwrap_or(base.splash),
        animations: file.animations.unwrap_or(base.animations),
        fps: file.fps.unwrap_or(base.fps),
        reasoning: file.reasoning.as_ref().map_or(base.reasoning, reasoning),
        receipts: file.receipts.as_deref().map_or(base.receipts, |text| {
            Visibility::parse(text, Visibility::Off)
        }),
        tools: file.tools.unwrap_or(base.tools),
        diff_layout: file.diff_layout.unwrap_or(base.diff_layout),
        layout: file.layout.clone().unwrap_or(base.layout),
        mail: file.mail.unwrap_or(base.mail),
        ban_comments: file.ban_comments.unwrap_or(base.ban_comments),
        ban_tests: file.ban_tests.unwrap_or(base.ban_tests),
        plan_mode: file.plan_mode.unwrap_or(base.plan_mode),
    })
}

fn reasoning(setting: &config::ReasoningSetting) -> Visibility {
    match setting {
        config::ReasoningSetting::Flag(true) => Visibility::Expanded,
        config::ReasoningSetting::Flag(false) => Visibility::Off,
        config::ReasoningSetting::Text(text) => Visibility::parse(text, Visibility::Collapsed),
    }
}

pub(crate) fn save_in(path: &Path, prefs: &Prefs) -> Result<()> {
    config::save_prefs(path, prefs)
}

#[must_use]
pub fn path() -> Option<std::path::PathBuf> {
    config::path().ok()
}

pub fn load() -> Result<Prefs> {
    from_file(&*config::file()?)
}
