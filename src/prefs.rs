use std::path::Path;

use crate::config;
use summonr_core::error::Result;

mod hotkeys;
pub use hotkeys::Hotkey;
pub(crate) use hotkeys::{EditorKey, Hotkeys};
pub(crate) use summonr_types::{Background, DiffLayout, Notify};

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

pub(crate) use summonr_types::prefs::{ToolOutput, Visibility};

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
