use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::Path;

use sigil::prelude::Rgba8;

use crate::error::{Error, Result};

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
        code_bg: rgb(30, 32, 42),
        diff_add_bg: rgb(18, 76, 28),
        diff_del_bg: rgb(84, 24, 32),
        code_comment: rgb(96, 148, 102),
        code_string: rgb(226, 178, 106),
        code_keyword: rgb(198, 146, 230),
        accent: rgb(105, 140, 210),
    };

    #[must_use]
    pub(super) fn named(name: &str) -> Self {
        let wanted = name.trim();
        Self::ALL
            .iter()
            .find(|(preset, _)| *preset == wanted)
            .map_or(Self::SPACE, |(_, palette)| *palette)
    }

    pub(super) const ALL: [(&'static str, Self); 6] = [
        ("space", Self::SPACE),
        ("fantasy", Self::FANTASY),
        ("nord", Self::NORD),
        ("gruvbox", Self::GRUVBOX),
        ("solarized", Self::SOLARIZED),
        ("light", Self::LIGHT),
    ];

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
        code_bg: rgb(52, 59, 74),
        diff_add_bg: rgb(59, 74, 63),
        diff_del_bg: rgb(74, 58, 63),
        code_comment: rgb(97, 110, 136),
        code_string: rgb(235, 203, 139),
        code_keyword: rgb(180, 142, 173),
        accent: rgb(136, 192, 208),
    };

    pub(super) const GRUVBOX: Self = Self {
        base: rgb(40, 40, 40),
        text: rgb(235, 219, 178),
        gray: rgb(213, 196, 161),
        dim: rgb(146, 131, 116),
        thought: rgb(189, 174, 147),
        green: rgb(184, 187, 38),
        red: rgb(251, 73, 52),
        cyan: rgb(142, 192, 124),
        yellow: rgb(250, 189, 47),
        error: rgb(251, 73, 52),
        black: rgb(29, 32, 33),
        user_bg: rgb(60, 56, 54),
        tool_bg: rgb(50, 48, 47),
        code_bg: rgb(60, 56, 54),
        diff_add_bg: rgb(50, 59, 38),
        diff_del_bg: rgb(64, 40, 42),
        code_comment: rgb(146, 131, 116),
        code_string: rgb(184, 187, 38),
        code_keyword: rgb(211, 134, 155),
        accent: rgb(215, 153, 33),
    };

    pub(super) const SOLARIZED: Self = Self {
        base: rgb(0, 43, 54),
        text: rgb(238, 232, 213),
        gray: rgb(147, 161, 161),
        dim: rgb(131, 148, 150),
        thought: rgb(143, 161, 161),
        green: rgb(133, 153, 0),
        red: rgb(220, 50, 47),
        cyan: rgb(42, 161, 152),
        yellow: rgb(181, 137, 0),
        error: rgb(220, 50, 47),
        black: rgb(0, 31, 39),
        user_bg: rgb(7, 54, 66),
        tool_bg: rgb(7, 54, 66),
        code_bg: rgb(7, 54, 66),
        diff_add_bg: rgb(13, 59, 46),
        diff_del_bg: rgb(59, 31, 31),
        code_comment: rgb(88, 110, 117),
        code_string: rgb(42, 161, 152),
        code_keyword: rgb(108, 113, 196),
        accent: rgb(38, 139, 210),
    };

    pub(super) const LIGHT: Self = Self {
        base: rgb(247, 244, 239),
        text: rgb(31, 36, 48),
        gray: rgb(74, 80, 96),
        dim: rgb(107, 114, 128),
        thought: rgb(91, 100, 115),
        green: rgb(31, 122, 61),
        red: rgb(179, 38, 30),
        cyan: rgb(15, 111, 134),
        yellow: rgb(138, 106, 0),
        error: rgb(179, 38, 30),
        black: rgb(230, 225, 216),
        user_bg: rgb(232, 227, 218),
        tool_bg: rgb(239, 234, 226),
        code_bg: rgb(234, 229, 220),
        diff_add_bg: rgb(215, 234, 217),
        diff_del_bg: rgb(243, 215, 213),
        code_comment: rgb(107, 114, 128),
        code_string: rgb(138, 106, 0),
        code_keyword: rgb(123, 63, 160),
        accent: rgb(29, 78, 216),
    };

    pub(super) const FANTASY: Self = Self {
        base: rgb(20, 16, 28),
        text: rgb(244, 239, 230),
        gray: rgb(198, 189, 208),
        dim: rgb(145, 138, 160),
        thought: rgb(169, 159, 180),
        green: rgb(127, 191, 106),
        red: rgb(217, 106, 106),
        cyan: rgb(106, 198, 217),
        yellow: rgb(224, 180, 92),
        error: rgb(209, 92, 92),
        black: rgb(0, 0, 0),
        user_bg: rgb(42, 35, 56),
        tool_bg: rgb(27, 22, 38),
        code_bg: rgb(34, 27, 46),
        diff_add_bg: rgb(30, 51, 38),
        diff_del_bg: rgb(58, 32, 40),
        code_comment: rgb(143, 154, 122),
        code_string: rgb(232, 192, 122),
        code_keyword: rgb(201, 143, 224),
        accent: rgb(212, 162, 76),
    };

    fn set(&mut self, key: &str, value: Rgba8) -> bool {
        match key {
            "base" | "background" => self.base = value,
            "text" | "foreground" => self.text = value,
            "gray" => self.gray = value,
            "dim" => self.dim = value,
            "thought" => self.thought = value,
            "green" => self.green = value,
            "red" => self.red = value,
            "cyan" => self.cyan = value,
            "yellow" => self.yellow = value,
            "error" => self.error = value,
            "black" => self.black = value,
            "user_bg" => self.user_bg = value,
            "tool_bg" => self.tool_bg = value,
            "code_bg" => self.code_bg = value,
            "diff_add_bg" => self.diff_add_bg = value,
            "diff_del_bg" => self.diff_del_bg = value,
            "code_comment" => self.code_comment = value,
            "code_string" => self.code_string = value,
            "code_keyword" => self.code_keyword = value,
            "accent" => self.accent = value,
            _ => return false,
        }
        true
    }
}

thread_local! {
    static ACTIVE: Cell<Palette> = const { Cell::new(Palette::SPACE) };
}

#[must_use]
pub(super) fn palette() -> Palette {
    ACTIVE.with(Cell::get)
}

pub(super) fn set(palette: Palette) {
    ACTIVE.with(|active| active.set(palette));
}

pub(super) fn install(name: &str, workspace: &Path, config: Option<&Path>) {
    let mut palette = Palette::named(name);
    if let Some(dir) = config.and_then(Path::parent) {
        overlay(&mut palette, &dir.join("theme.toml"));
    }
    overlay(&mut palette, &workspace.join(".summonr/theme.toml"));
    set(palette);
}

fn overlay(palette: &mut Palette, path: &Path) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    let Ok(rows) = toml::from_str::<BTreeMap<String, String>>(&text) else {
        return;
    };
    for (key, value) in rows {
        if let Ok(color) = hex(&value) {
            palette.set(key.trim(), color);
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
