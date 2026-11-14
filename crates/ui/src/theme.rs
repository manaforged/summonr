use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::Path;

use sigil::prelude::Color;

use super::canvas::rgb;
use summonr_agent::durable::store::{self, Durability};
use summonr_agent::prefs::{Background, Prefs};
use summonr_core::error::{Error, Result};

pub(crate) mod tokens;
pub(crate) use tokens::style;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Palette {
    pub(super) base: Color,
    pub(super) text: Color,
    pub(super) gray: Color,
    pub(super) dim: Color,
    pub(super) thought: Color,
    pub(super) green: Color,
    pub(super) red: Color,
    pub(super) cyan: Color,
    pub(super) yellow: Color,
    pub(super) error: Color,
    pub(super) black: Color,
    pub(super) user_bg: Color,
    pub(super) tool_bg: Color,
    pub(super) code_bg: Color,
    pub(super) diff_add_bg: Color,
    pub(super) diff_del_bg: Color,
    pub(super) code_comment: Color,
    pub(super) code_string: Color,
    pub(super) code_keyword: Color,
    pub(super) accent: Color,
}

impl Palette {
    pub(super) const SPACE: Self = Self {
        base: rgb(12, 12, 20),
        text: rgb(255, 255, 255),
        gray: rgb(188, 194, 210),
        dim: rgb(148, 156, 176),
        thought: rgb(168, 176, 196),
        green: rgb(80, 200, 120),
        red: rgb(225, 95, 95),
        cyan: rgb(90, 200, 220),
        yellow: rgb(220, 200, 90),
        error: rgb(220, 90, 90),
        black: rgb(0, 0, 0),
        user_bg: rgb(42, 44, 58),
        tool_bg: rgb(24, 26, 36),
        code_bg: rgb(40, 42, 56),
        diff_add_bg: rgb(18, 76, 28),
        diff_del_bg: rgb(84, 24, 32),
        code_comment: rgb(96, 148, 102),
        code_string: rgb(226, 178, 106),
        code_keyword: rgb(198, 146, 230),
        accent: rgb(105, 140, 210),
    };

    #[must_use]
    pub(super) fn named(name: &str) -> Self {
        let wanted = match summonr_agent::prefs::theme(name) {
            summonr_agent::prefs::AUTO_THEME => {
                match (super::canvas::is_window(), backdrop_light()) {
                    (true, true) => "paper",
                    (true, false) => "iron",
                    (false, true) => "dawn",
                    (false, false) => "space",
                }
            }
            known => known,
        };
        Self::ALL
            .iter()
            .find(|(preset, _)| *preset == wanted)
            .map_or(Self::SPACE, |(_, palette)| *palette)
    }

    pub(super) const ALL: [(&'static str, Self); 5] = [
        ("space", Self::SPACE),
        ("nord", Self::NORD),
        ("dawn", Self::DAWN),
        ("paper", Self::PAPER),
        ("iron", Self::IRON),
    ];

    /// The summonr.dev light palette: warm paper, iron ink, spark-green accent.
    pub(super) const PAPER: Self = Self {
        base: rgb(251, 250, 247),
        text: rgb(28, 29, 26),
        gray: rgb(43, 44, 40),
        dim: rgb(92, 90, 84),
        thought: rgb(95, 93, 87),
        green: rgb(61, 122, 64),
        red: rgb(176, 58, 46),
        cyan: rgb(40, 110, 128),
        yellow: rgb(168, 122, 18),
        error: rgb(176, 58, 46),
        black: rgb(242, 239, 232),
        user_bg: rgb(244, 242, 236),
        tool_bg: rgb(244, 242, 236),
        code_bg: rgb(220, 216, 208),
        diff_add_bg: rgb(229, 239, 214),
        diff_del_bg: rgb(246, 226, 221),
        code_comment: rgb(95, 93, 87),
        code_string: rgb(140, 98, 20),
        code_keyword: rgb(110, 70, 140),
        accent: rgb(79, 102, 38),
    };

    /// The summonr.dev dark palette: iron ground, bone text, spark accent.
    pub(super) const IRON: Self = Self {
        base: rgb(28, 29, 26),
        text: rgb(236, 231, 220),
        gray: rgb(220, 215, 204),
        dim: rgb(169, 164, 154),
        thought: rgb(162, 157, 147),
        green: rgb(150, 196, 120),
        red: rgb(224, 122, 106),
        cyan: rgb(120, 190, 200),
        yellow: rgb(217, 183, 90),
        error: rgb(224, 122, 106),
        black: rgb(23, 24, 21),
        user_bg: rgb(35, 36, 31),
        tool_bg: rgb(32, 33, 29),
        code_bg: rgb(52, 53, 48),
        diff_add_bg: rgb(39, 51, 28),
        diff_del_bg: rgb(58, 36, 32),
        code_comment: rgb(162, 157, 147),
        code_string: rgb(217, 183, 90),
        code_keyword: rgb(190, 160, 210),
        accent: rgb(201, 218, 165),
    };

    pub(super) const DAWN: Self = Self {
        base: rgb(222, 218, 208),
        text: rgb(0, 0, 0),
        gray: rgb(62, 68, 82),
        dim: rgb(70, 76, 88),
        thought: rgb(72, 78, 90),
        green: rgb(38, 94, 56),
        red: rgb(140, 50, 42),
        cyan: rgb(20, 86, 100),
        yellow: rgb(100, 76, 16),
        error: rgb(140, 50, 42),
        black: rgb(204, 199, 187),
        user_bg: rgb(208, 203, 191),
        tool_bg: rgb(215, 210, 199),
        code_bg: rgb(192, 186, 172),
        diff_add_bg: rgb(196, 214, 196),
        diff_del_bg: rgb(226, 200, 195),
        code_comment: rgb(70, 76, 88),
        code_string: rgb(100, 76, 16),
        code_keyword: rgb(104, 62, 136),
        accent: rgb(40, 68, 128),
    };

    pub(super) const NORD: Self = Self {
        base: rgb(46, 52, 64),
        text: rgb(236, 239, 244),
        gray: rgb(216, 222, 233),
        dim: rgb(129, 161, 193),
        thought: rgb(154, 167, 189),
        green: rgb(163, 190, 140),
        red: rgb(191, 97, 106),
        cyan: rgb(136, 192, 208),
        yellow: rgb(235, 203, 139),
        error: rgb(191, 97, 106),
        black: rgb(36, 41, 51),
        user_bg: rgb(59, 66, 82),
        tool_bg: rgb(43, 49, 61),
        code_bg: rgb(67, 76, 94),
        diff_add_bg: rgb(59, 74, 63),
        diff_del_bg: rgb(74, 58, 63),
        code_comment: rgb(130, 145, 170),
        code_string: rgb(235, 203, 139),
        code_keyword: rgb(180, 142, 173),
        accent: rgb(129, 161, 193),
    };

    pub(super) fn set(&mut self, key: &str, value: Color) -> bool {
        let key = ALIASES
            .iter()
            .find(|(alias, _)| *alias == key)
            .map_or(key, |(_, key)| key);
        let Some((_, slot)) = SLOTS.iter().find(|(name, _)| *name == key) else {
            return false;
        };
        *slot(self) = value;
        true
    }
}

impl Palette {
    /// Whether the base reads as paper rather than night.
    #[must_use]
    pub(crate) fn light(self) -> bool {
        let [r, g, b] = [self.base.r, self.base.g, self.base.b].map(u32::from);
        r * 299 + g * 587 + b * 114 > 128_000
    }

    pub(super) fn colors(mut self) -> Vec<(&'static str, Color)> {
        SLOTS
            .iter()
            .map(|(name, slot)| (*name, *slot(&mut self)))
            .collect()
    }

    pub(super) fn save(self, path: &Path) -> Result<()> {
        let source = store::read_or_empty(path)?;
        let mut document = source
            .parse::<toml_edit::DocumentMut>()
            .map_err(|error| Error::Config(format!("{}: {error}", path.display())))?;
        for (name, color) in self.colors() {
            document[name] = toml_edit::value(color_hex(color));
        }
        store::write(path, document.to_string(), Durability::Synced)?;
        Ok(())
    }
}

pub(super) fn color_hex(color: Color) -> String {
    format!(
        "#{:02x}{:02x}{:02x}{:02x}",
        color.r, color.g, color.b, color.a
    )
}

type Slot = fn(&mut Palette) -> &mut Color;

const ALIASES: [(&str, &str); 2] = [("background", "base"), ("foreground", "text")];

const SLOTS: [(&str, Slot); 20] = [
    ("base", |palette| &mut palette.base),
    ("text", |palette| &mut palette.text),
    ("gray", |palette| &mut palette.gray),
    ("dim", |palette| &mut palette.dim),
    ("thought", |palette| &mut palette.thought),
    ("green", |palette| &mut palette.green),
    ("red", |palette| &mut palette.red),
    ("cyan", |palette| &mut palette.cyan),
    ("yellow", |palette| &mut palette.yellow),
    ("error", |palette| &mut palette.error),
    ("black", |palette| &mut palette.black),
    ("user_bg", |palette| &mut palette.user_bg),
    ("tool_bg", |palette| &mut palette.tool_bg),
    ("code_bg", |palette| &mut palette.code_bg),
    ("diff_add_bg", |palette| &mut palette.diff_add_bg),
    ("diff_del_bg", |palette| &mut palette.diff_del_bg),
    ("code_comment", |palette| &mut palette.code_comment),
    ("code_string", |palette| &mut palette.code_string),
    ("code_keyword", |palette| &mut palette.code_keyword),
    ("accent", |palette| &mut palette.accent),
];

thread_local! {
    static ACTIVE: Cell<Palette> = const { Cell::new(Palette::SPACE) };
    static LIGHT_BACKDROP: Cell<Option<bool>> = const { Cell::new(None) };
}

#[cfg(test)]
pub(super) fn set_backdrop_light(light: bool) {
    LIGHT_BACKDROP.with(|slot| slot.set(Some(light)));
}

fn backdrop_light() -> bool {
    LIGHT_BACKDROP.with(Cell::get).unwrap_or_else(|| {
        let backdrop = sigil::terminal::backdrop()
            .peek()
            .or_else(|| sigil::terminal::caps().and_then(|caps| caps.backdrop()));
        backdrop == Some(sigil::terminal::Backdrop::Light)
    })
}

/// The opaque colour behind the UI: the theme base, or the colour the terminal reports when the base is see-through.
#[must_use]
pub(crate) fn backdrop(palette: Palette) -> sigil::prelude::Color {
    let backdrop = if palette.base.a == 0 {
        sigil::terminal::caps()
            .and_then(|caps| caps.background())
            .unwrap_or(palette.base)
    } else {
        palette.base
    };
    sigil::prelude::Color { a: 255, ..backdrop }
}

#[must_use]
pub(super) fn palette() -> Palette {
    ACTIVE.with(Cell::get)
}

pub(super) fn set(palette: Palette) {
    ACTIVE.with(|active| active.set(palette));
}

#[must_use]
pub(super) fn install(prefs: &Prefs, workspace: &Path, config: Option<&Path>) -> Vec<String> {
    let (mut palette, problems) = load(prefs, workspace, config);
    if prefs.background == Background::Terminal {
        palette.base.a = 0;
    }
    set(palette);
    problems
}

pub(super) fn load(
    prefs: &Prefs,
    workspace: &Path,
    config: Option<&Path>,
) -> (Palette, Vec<String>) {
    let mut palette = Palette::named(&prefs.theme);
    let mut problems = Vec::new();
    if let Some(dir) = config.and_then(Path::parent) {
        overlay(&mut palette, &dir.join("theme.toml"), &mut problems);
    }
    overlay(
        &mut palette,
        &summonr_core::dirs::workspace(workspace).join("theme.toml"),
        &mut problems,
    );
    (palette, problems)
}

fn overlay(palette: &mut Palette, path: &Path, problems: &mut Vec<String>) {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => {
            problems.push(format!("cannot read {}: {error}", path.display()));
            return;
        }
    };
    let rows = match toml::from_str::<BTreeMap<String, String>>(&text) {
        Ok(rows) => rows,
        Err(error) => {
            problems.push(format!("{}: {error}", path.display()));
            return;
        }
    };
    let file = path.display();
    for (key, value) in rows {
        let key = key.trim();
        match hex(&value) {
            Ok(color) if palette.set(key, color) => {}
            Ok(_) => problems.push(format!("{file}: unknown theme key `{key}`")),
            Err(error) => problems.push(format!("{file}: {error}")),
        }
    }
}

pub(super) fn hex(value: &str) -> Result<Color> {
    let digits = value.trim();
    let digits = digits.strip_prefix('#').unwrap_or(digits);
    let invalid = || {
        Error::Config(format!(
            "theme color `{value}` must be #rgb, #rrggbb, or #rrggbbaa"
        ))
    };
    if !digits.is_ascii() {
        return Err(invalid());
    }
    let byte = |range: std::ops::Range<usize>| -> Result<u8> {
        u8::from_str_radix(&digits[range], 16).map_err(|_error| invalid())
    };
    let color = match digits.len() {
        3 => {
            let nibble = |index: usize| -> Result<u8> {
                let digit = u8::from_str_radix(&digits[index..index + 1], 16)
                    .map_err(|_error| invalid())?;
                Ok(digit * 17)
            };
            Color {
                r: nibble(0)?,
                g: nibble(1)?,
                b: nibble(2)?,
                a: 255,
            }
        }
        6 | 8 => Color {
            r: byte(0..2)?,
            g: byte(2..4)?,
            b: byte(4..6)?,
            a: if digits.len() == 8 { byte(6..8)? } else { 255 },
        },
        _ => return Err(invalid()),
    };
    Ok(color)
}

#[cfg(test)]
#[path = "theme_tests.rs"]
mod tests;
