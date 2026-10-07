use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::Path;

use sigil::prelude::Rgba8;

use crate::error::{Error, Result};
use crate::prefs::{Background, Prefs};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Palette {
    pub(super) base: Rgba8,
    pub(super) text: Rgba8,
    pub(super) gray: Rgba8,
    pub(super) dim: Rgba8,
    pub(super) thought: Rgba8,
    pub(super) green: Rgba8,
    pub(super) red: Rgba8,
    pub(super) cyan: Rgba8,
    pub(super) yellow: Rgba8,
    pub(super) error: Rgba8,
    pub(super) black: Rgba8,
    pub(super) user_bg: Rgba8,
    pub(super) tool_bg: Rgba8,
    pub(super) code_bg: Rgba8,
    pub(super) diff_add_bg: Rgba8,
    pub(super) diff_del_bg: Rgba8,
    pub(super) code_comment: Rgba8,
    pub(super) code_string: Rgba8,
    pub(super) code_keyword: Rgba8,
    pub(super) accent: Rgba8,
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
        let wanted = match crate::prefs::theme(name) {
            crate::prefs::AUTO_THEME if backdrop_light() => "dawn",
            crate::prefs::AUTO_THEME => "space",
            known => known,
        };
        Self::ALL
            .iter()
            .find(|(preset, _)| *preset == wanted)
            .map_or(Self::SPACE, |(_, palette)| *palette)
    }

    pub(super) const ALL: [(&'static str, Self); 3] = [
        ("space", Self::SPACE),
        ("nord", Self::NORD),
        ("dawn", Self::DAWN),
    ];

    pub(super) const DAWN: Self = Self {
        base: rgb(222, 218, 208),
        text: rgb(46, 52, 64),
        gray: rgb(76, 82, 96),
        dim: rgb(84, 90, 102),
        thought: rgb(92, 98, 110),
        green: rgb(52, 112, 72),
        red: rgb(160, 64, 56),
        cyan: rgb(30, 104, 120),
        yellow: rgb(128, 100, 30),
        error: rgb(160, 64, 56),
        black: rgb(204, 199, 187),
        user_bg: rgb(208, 203, 191),
        tool_bg: rgb(215, 210, 199),
        code_bg: rgb(192, 186, 172),
        diff_add_bg: rgb(196, 214, 196),
        diff_del_bg: rgb(226, 200, 195),
        code_comment: rgb(84, 90, 102),
        code_string: rgb(128, 100, 30),
        code_keyword: rgb(116, 72, 148),
        accent: rgb(56, 92, 160),
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

    fn set(&mut self, key: &str, value: Rgba8) -> bool {
        let Some((_, slot)) = SLOTS.iter().find(|(name, _)| *name == key) else {
            return false;
        };
        *slot(self) = value;
        true
    }
}

type Slot = fn(&mut Palette) -> &mut Rgba8;

const SLOTS: [(&str, Slot); 22] = [
    ("base", |palette| &mut palette.base),
    ("background", |palette| &mut palette.base),
    ("text", |palette| &mut palette.text),
    ("foreground", |palette| &mut palette.text),
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
        matches!(
            sigil::terminal::caps().and_then(|caps| caps.backdrop),
            Some(sigil::terminal::Backdrop::Light)
        )
    })
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
    let mut palette = Palette::named(&prefs.theme);
    let mut problems = Vec::new();
    if let Some(dir) = config.and_then(Path::parent) {
        overlay(&mut palette, &dir.join("theme.toml"), &mut problems);
    }
    overlay(
        &mut palette,
        &workspace.join(".summonr/theme.toml"),
        &mut problems,
    );
    if prefs.background == Background::Terminal {
        palette.base.a = 0;
    }
    set(palette);
    problems
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

pub(super) fn hex(value: &str) -> Result<Rgba8> {
    let digits = value.trim();
    let digits = digits.strip_prefix('#').unwrap_or(digits);
    let invalid = || {
        Error::Config(format!(
            "theme color `{value}` must be #rgb, #rrggbb, or #rrggbbaa"
        ))
    };
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
            Rgba8 {
                r: nibble(0)?,
                g: nibble(1)?,
                b: nibble(2)?,
                a: 255,
            }
        }
        6 | 8 => Rgba8 {
            r: byte(0..2)?,
            g: byte(2..4)?,
            b: byte(4..6)?,
            a: if digits.len() == 8 { byte(6..8)? } else { 255 },
        },
        _ => return Err(invalid()),
    };
    Ok(color)
}

const fn rgb(r: u8, g: u8, b: u8) -> Rgba8 {
    Rgba8 { r, g, b, a: 255 }
}

#[cfg(test)]
#[path = "theme_tests.rs"]
mod tests;
