use std::collections::BTreeMap;

use super::galaxy::field;
use super::space::{DOT, braille, hash, index, under};
use super::*;

const SALT: u16 = 0x2a71;

const AREA: u32 = 5;

const DENSITY: [f32; 2] = [0.25, 1.1];

const SIZES: [f32; 3] = [0.74, 0.94, 0.992];

const LIGHT: [f32; 4] = [0.55, 0.78, 1.0, 1.0];

const SPREAD: f32 = 0.2;

const GLYPH: [Option<&str>; 4] = [None, None, Some("·"), Some("•")];

const TINTS: [Rgba8; 10] = [
    rgb(170, 200, 255),
    rgb(200, 216, 255),
    rgb(228, 232, 248),
    rgb(240, 240, 244),
    rgb(240, 240, 244),
    rgb(248, 240, 222),
    rgb(255, 230, 196),
    rgb(255, 230, 196),
    rgb(255, 214, 170),
    rgb(240, 238, 248),
];

const TWINKLE_ODDS: [u32; 2] = [9, 3];

const PERIOD: [f32; 2] = [75.0, 125.0];

const TWINKLE: [f32; 4] = [0.55, 0.75, 0.9, 1.0];

pub(super) struct Shine {
    pub(super) x: u16,
    pub(super) y: u16,
    pub(super) text: &'static str,
    pub(super) fg: Rgba8,
    pub(super) bg: Rgba8,
}

struct Lit {
    bits: u8,
    size: usize,
    tint: Rgba8,
    light: f32,
    twinkle: Option<[f32; 2]>,
}

pub(super) fn still(cols: u16, rows: u16, seed: u64) -> Vec<Shine> {
    let field = field(cols, rows, seed);
    catalogue(cols, rows, seed)
        .into_iter()
        .filter(|(_, lit)| lit.twinkle.is_none())
        .map(|((x, y), lit)| shine(x, y, &lit, under(&field[index(cols, x, y)]), 1.0))
        .collect()
}

pub(super) fn twinkling(cols: u16, rows: u16, beat: f64, seed: u64) -> Vec<Shine> {
    let field = field(cols, rows, seed);
    catalogue(cols, rows, seed)
        .into_iter()
        .filter_map(|((x, y), lit)| {
            let [period, phase] = lit.twinkle?;
            let wave = 0.5 + 0.5 * (beat as f32 / period * std::f32::consts::TAU + phase).sin();
            let level = TWINKLE[((wave * TWINKLE.len() as f32) as usize).min(TWINKLE.len() - 1)];
            Some(shine(x, y, &lit, under(&field[index(cols, x, y)]), level))
        })
        .collect()
}

pub(super) fn bright(cols: u16, rows: u16, seed: u64) -> Vec<(u16, u16)> {
    catalogue(cols, rows, seed)
        .into_iter()
        .filter(|(_, lit)| GLYPH[lit.size].is_some())
        .map(|(at, _)| at)
        .collect()
}

fn catalogue(cols: u16, rows: u16, seed: u64) -> BTreeMap<(u16, u16), Lit> {
    let mut lit: BTreeMap<(u16, u16), Lit> = BTreeMap::new();
    if cols == 0 || rows == 0 {
        return lit;
    }
    let field = field(cols, rows, seed);
    for star in 0..u32::from(cols) * u32::from(rows) / AREA {
        let at = hash(star as u16, SALT ^ seed as u16);
        let roll = hash(at as u16, (at >> 16) as u16);
        let x = at % (u32::from(cols) * 2);
        let y = (at >> 12) % (u32::from(rows) * 4);
        let cell = ((x / 2) as u16, (y / 4) as u16);
        let glow = luminance(under(&field[index(cols, cell.0, cell.1)]));
        if fraction(roll, 1000) > DENSITY[0] + DENSITY[1] * glow {
            continue;
        }
        let pick = fraction(roll >> 10, 10_000);
        let size = SIZES.partition_point(|edge| *edge <= pick);
        let twinkles = (roll >> 20).is_multiple_of(TWINKLE_ODDS[0])
            || GLYPH[size].is_some() && (roll >> 20).is_multiple_of(TWINKLE_ODDS[1]);
        let star = Lit {
            bits: DOT[(y % 4 * 2 + x % 2) as usize],
            size,
            tint: TINTS[(roll >> 24) as usize % TINTS.len()],
            light: LIGHT[size] * (1.0 - SPREAD + SPREAD * fraction(roll >> 4, 100)),
            twinkle: twinkles.then(|| {
                [
                    PERIOD[0] + PERIOD[1] * fraction(roll >> 2, 100),
                    fraction(roll, 997) * std::f32::consts::TAU,
                ]
            }),
        };
        let entry = lit.entry(cell).or_insert(Lit { bits: 0, ..star });
        let bits = entry.bits | star.bits;
        if star.size >= entry.size {
            *entry = Lit { bits, ..star };
        } else {
            entry.bits = bits;
        }
    }
    lit
}

fn shine(x: u16, y: u16, lit: &Lit, bg: Rgba8, level: f32) -> Shine {
    Shine {
        x,
        y,
        text: GLYPH[lit.size].unwrap_or_else(|| braille(lit.bits)),
        fg: blend(bg, lit.tint, lit.light * level),
        bg,
    }
}

fn luminance(colour: Rgba8) -> f32 {
    (0.3 * f32::from(colour.r) + 0.55 * f32::from(colour.g) + 0.15 * f32::from(colour.b)) / 255.0
}

fn fraction(bits: u32, modulo: u32) -> f32 {
    (bits % modulo) as f32 / modulo as f32
}
