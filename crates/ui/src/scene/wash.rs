use super::galaxy::Sample;
use super::space::hash;
use super::*;

/// The pigments of the light sky, shared with the desktop wash.
pub(crate) const DAY: [Color; 4] = [
    rgb(64, 132, 214),
    rgb(92, 96, 206),
    rgb(168, 82, 190),
    rgb(226, 88, 138),
];

const GAIN: f32 = 2.0;
const FLOOR: f32 = 0.2;
const POOLS: f32 = 4.0;
const UNDERWASH: f32 = 0.62;
const RIM: [f32; 3] = [0.07, 0.03, 0.34];
const GRAIN: [f32; 2] = [0.42, 0.26];
const HUE: [f32; 2] = [0.39, 0.22];
const CORE: f32 = 0.55;
const NEBULA: [f32; 2] = [0.45, 0.6];

pub(super) fn paint(samples: &[Sample], cols: u16, paper: Color) -> Vec<Color> {
    let width = usize::from(cols);
    let amounts: Vec<f32> = samples.iter().map(amount).collect();
    let at = |x: isize, y: isize| {
        if x < 0 || y < 0 || x >= width as isize {
            return 0.0;
        }
        amounts
            .get(y as usize * width + x as usize)
            .copied()
            .unwrap_or(0.0)
    };
    samples
        .iter()
        .enumerate()
        .map(|(index, sample)| {
            let (x, y) = ((index % width) as isize, (index / width) as isize);
            let held = amounts[index];
            if held <= 0.0 {
                return paper;
            }
            let jump = [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)]
                .into_iter()
                .map(|(nx, ny)| held - at(nx, ny))
                .fold(0.0, f32::max);
            let rim = if jump > RIM[0] {
                RIM[2] * ((jump - RIM[1]) / 0.16).min(1.0)
            } else {
                0.0
            };
            let roll = (hash(x as u16, y as u16) >> 16) as f32 / f32::from(u16::MAX) - 0.5;
            let deep = ((held - GRAIN[0]) / 0.4).clamp(0.0, 1.0);
            let wet = ((held + rim) * (1.0 + GRAIN[1] * roll * deep)).clamp(0.0, 1.0);
            dye(paper, pigment(sample), wet)
        })
        .collect()
}

fn amount(sample: &Sample) -> f32 {
    let body = ((sample.shade - FLOOR) * GAIN).clamp(0.0, 1.0);
    let scaled = body * POOLS;
    let rest = scaled.fract();
    let settle = ((rest - 0.8) / 0.2).clamp(0.0, 1.0);
    let settled = (scaled.floor() + settle * settle * (3.0 - 2.0 * settle)) / POOLS;
    settled.max(UNDERWASH * body.sqrt())
}

fn pigment(sample: &Sample) -> Color {
    let at = ((sample.hue - HUE[0]) / HUE[1]).clamp(0.0, 1.0) * (DAY.len() - 1) as f32;
    let index = (at as usize).min(DAY.len() - 2);
    let base = blend(DAY[index], DAY[index + 1], at - index as f32);
    let cored = blend(
        base,
        DAY[3],
        ((sample.glow - 0.35) * 1.5).clamp(0.0, 1.0) * CORE,
    );
    if sample.nebula > NEBULA[0] {
        blend(cored, DAY[2], NEBULA[1])
    } else {
        cored
    }
}

fn dye(paper: Color, pigment: Color, amount: f32) -> Color {
    let channel = |paper: u8, pigment: u8| {
        let kept = 1.0 - amount + f32::from(pigment) / 255.0 * amount;
        (f32::from(paper) * kept).round() as u8
    };
    rgb(
        channel(paper.r, pigment.r),
        channel(paper.g, pigment.g),
        channel(paper.b, pigment.b),
    )
}
