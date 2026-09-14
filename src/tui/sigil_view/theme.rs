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
    pub(super) thought_bg: Rgba8,
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
        text: rgb(200, 200, 205),
        gray: rgb(160, 160, 160),
        dim: rgb(90, 90, 90),
        thought: rgb(186, 186, 192),
        green: rgb(80, 200, 120),
        red: rgb(225, 95, 95),
        cyan: rgb(90, 200, 220),
        yellow: rgb(220, 200, 90),
        error: rgb(220, 90, 90),
        black: rgb(0, 0, 0),
        user_bg: rgb(42, 44, 58),
        tool_bg: rgb(24, 26, 36),
        code_bg: rgb(30, 32, 42),
        thought_bg: rgb(36, 36, 44),
        diff_add_bg: rgb(24, 48, 32),
        diff_del_bg: rgb(56, 28, 32),
        code_comment: rgb(96, 148, 102),
        code_string: rgb(226, 178, 106),
        code_keyword: rgb(198, 146, 230),
        accent: rgb(105, 140, 210),
    };

    pub(super) const FANTASY: Self = Self {
        accent: rgb(255, 208, 96),
        ..Self::SPACE
    };

    #[must_use]
    pub(super) fn named(name: &str) -> Self {
        if name == "fantasy" {
            Self::FANTASY
        } else {
            Self::SPACE
        }
    }

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
            "thought_bg" => self.thought_bg = value,
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
    overlay(&mut palette, &workspace.join(".mage/theme.toml"));
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

#[cfg(test)]
#[path = "theme_tests.rs"]
mod tests;
