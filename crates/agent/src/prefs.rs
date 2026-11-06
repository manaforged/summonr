use std::path::Path;

use crate::config;
use summonr_core::error::Result;

mod hotkeys;
pub use hotkeys::Hotkey;
pub use hotkeys::{EditorKey, Hotkeys};
pub use summonr_types::{Background, DiffLayout, Notify};

pub const AUTO_THEME: &str = "auto";
pub const THEMES: [&str; 6] = ["space", "nord", "dawn", "paper", "iron", AUTO_THEME];
const RETIRED_THEMES: [(&str, &str); 4] = [
    ("light", "dawn"),
    ("fantasy", "space"),
    ("gruvbox", "space"),
    ("solarized", "space"),
];

pub fn theme(name: &str) -> &'static str {
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

pub use summonr_types::prefs::{ToolOutput, Visibility};

pub const FPS_DEFAULT: u32 = 25;
pub const FPS_MIN: u32 = 1;
pub const FPS_MAX: u32 = 240;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Spinner {
    #[default]
    Dots,
    Symbols,
}

impl Spinner {
    pub fn name(self) -> &'static str {
        match self {
            Self::Dots => "dots",
            Self::Symbols => "symbols",
        }
    }

    pub fn toggle(self) -> Self {
        match self {
            Self::Dots => Self::Symbols,
            Self::Symbols => Self::Dots,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prefs {
    pub theme: String,
    pub background: Background,
    pub notify: Notify,
    pub splash: bool,
    pub animations: bool,
    pub spinner: Spinner,
    pub fps: u32,
    pub reasoning: Visibility,
    pub receipts: Visibility,
    pub tools: ToolOutput,
    pub diff_layout: DiffLayout,
    pub mail: bool,
    pub layout: String,
    pub hotkeys: Hotkeys,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            theme: AUTO_THEME.to_string(),
            background: Background::default(),
            notify: Notify::Turn,
            splash: false,
            animations: true,
            spinner: Spinner::default(),
            fps: FPS_DEFAULT,
            reasoning: Visibility::Collapsed,
            receipts: Visibility::Off,
            tools: ToolOutput::Compact,
            diff_layout: DiffLayout::Split,
            mail: true,
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
        spinner: file.spinner.unwrap_or(base.spinner),
        fps: file.fps.unwrap_or(base.fps),
        reasoning: file.reasoning.as_ref().map_or(base.reasoning, reasoning),
        receipts: file.receipts.as_deref().map_or(base.receipts, |text| {
            Visibility::parse(text, Visibility::Off)
        }),
        tools: file.tools.unwrap_or(base.tools),
        diff_layout: file.diff_layout.unwrap_or(base.diff_layout),
        layout: file.layout.clone().unwrap_or(base.layout),
        mail: file.mail.unwrap_or(base.mail),
    })
}

fn reasoning(setting: &config::ReasoningSetting) -> Visibility {
    match setting {
        config::ReasoningSetting::Flag(true) => Visibility::Expanded,
        config::ReasoningSetting::Flag(false) => Visibility::Off,
        config::ReasoningSetting::Text(text) => Visibility::parse(text, Visibility::Collapsed),
    }
}

pub fn save_in(path: &Path, prefs: &Prefs) -> Result<()> {
    let before = from_file(&config::parse(path)?).unwrap_or_default();
    config::save_prefs(path, &before, prefs)
}

#[must_use]
pub fn path() -> Option<std::path::PathBuf> {
    config::path().ok()
}

pub fn load() -> Result<Prefs> {
    from_file(&*config::file()?)
}
