//! The shared cell canvas: one cell type and palette for every surface.
//!
//! Chat, settings, and the splash scene paint into `Vec<Cell>` grids and
//! hand them to their own row renderers. This module owns the cell type,
//! the blank, the color helper, and the palette both surfaces share.

use sigil::prelude::*;

pub(super) const fn rgb(r: u8, g: u8, b: u8) -> Rgba8 {
    Rgba8 { r, g, b, a: 255 }
}

pub(super) const BASE: Rgba8 = rgb(12, 12, 20);
pub(super) const TEXT: Rgba8 = rgb(200, 200, 205);
pub(super) const GRAY: Rgba8 = rgb(160, 160, 160);
pub(super) const DIM: Rgba8 = rgb(90, 90, 90);
pub(super) const GREEN: Rgba8 = rgb(80, 200, 120);
pub(super) const CYAN: Rgba8 = rgb(90, 200, 220);

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Cell {
    pub(super) ch: char,
    pub(super) fg: Rgba8,
    pub(super) bg: Rgba8,
    pub(super) bold: bool,
    pub(super) italic: bool,
}

impl Cell {
    pub(super) const fn blank() -> Self {
        Self {
            ch: ' ',
            fg: TEXT,
            bg: BASE,
            bold: false,
            italic: false,
        }
    }
}

#[cfg(test)]
#[path = "canvas_tests.rs"]
mod tests;
