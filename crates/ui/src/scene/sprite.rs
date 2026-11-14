use std::collections::VecDeque;
use std::f32::consts::TAU;

mod shape;

use shape::{BANNER, COMPACT, Lid, Shape, Tongue};

/// One pixel of the pixel Will: two pixels stack in each terminal cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Pixel {
    Empty,
    Edge,
    Mid,
    Core,
    Eye,
}

/// The two drawn sizes: 16×16 for the lockup, 9×10 when the terminal is short.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Size {
    Banner,
    Compact,
}

/// Milliseconds each idle frame holds.
pub(crate) const FRAME_MS: u64 = 45;
const CYCLE: u64 = 24;
const BLINK_EVERY: u64 = 4;
const LIDS: [Lid; 6] = [Lid::Half, Lid::Half, Lid::Shut, Lid::Shut, Lid::Half, Lid::Half];

impl Size {
    const fn shape(self) -> &'static Shape {
        match self {
            Self::Banner => &BANNER,
            Self::Compact => &COMPACT,
        }
    }

    /// Terminal columns and rows one frame covers.
    pub(crate) fn cells(self) -> [u16; 2] {
        let shape = self.shape();
        let rows = shape.grid.len() + shape.head + shape.foot;
        [shape.grid[0].len() as u16, rows.div_ceil(2) as u16]
    }
}

/// Frame `frame` of Will's idle loop, top row first; `still` holds the first frame.
pub(crate) fn frame(size: Size, frame: u64, still: bool) -> Vec<Vec<Pixel>> {
    let frame = if still { 0 } else { frame };
    let shape = size.shape();
    let lid = if (frame / CYCLE) % BLINK_EVERY == BLINK_EVERY - 1 {
        LIDS.get((frame % CYCLE) as usize).copied().unwrap_or(Lid::Open)
    } else {
        Lid::Open
    };
    animate(shape, &drawn(shape, lid), frame % CYCLE)
}

const GLYPHS: [(u8, Pixel); 4] = [
    (b'e', Pixel::Edge),
    (b'm', Pixel::Mid),
    (b'c', Pixel::Core),
    (b'o', Pixel::Eye),
];

fn pixel(byte: u8) -> Pixel {
    GLYPHS
        .iter()
        .find(|(glyph, _)| *glyph == byte)
        .map_or(Pixel::Empty, |(_, pixel)| *pixel)
}

fn drawn(shape: &Shape, lid: Lid) -> Vec<Vec<Pixel>> {
    let mut grid: Vec<Vec<Pixel>> =
        shape.grid.iter().map(|row| row.bytes().map(pixel).collect()).collect();
    let [x, top, height] = shape.eyes;
    let rows = match lid {
        Lid::Open => top..top + height,
        Lid::Half => top + 1..top + height,
        Lid::Shut => top + height - 1..top + height,
    };
    for column in [x, shape.mirror - x] {
        for row in rows.clone() {
            if let Some(cell) = grid.get_mut(row as usize).and_then(|line| line.get_mut(column as usize))
                && *cell != Pixel::Empty
            {
                *cell = Pixel::Eye;
            }
        }
    }
    grid
}

fn round(value: f32) -> i32 {
    (value + 0.5).floor() as i32
}

fn beat(t: u64, phase: f32) -> f32 {
    (TAU * t as f32 / CYCLE as f32 + phase).sin()
}

fn tongue_rows(tongue: &Tongue, bottom: i32, t: u64) -> Vec<[i32; 3]> {
    let n = tongue.rows.len();
    let s = 1.0 + tongue.stretch * beat(t, tongue.phase);
    let height = round(n as f32 * s).max(tongue.min as i32) as usize;
    let mut out = Vec::with_capacity(height);
    let mut previous: Option<[i32; 2]> = None;
    for k in 0..height {
        let source = (k as f32 / s).min((n - 1) as f32);
        let low = source.floor() as usize;
        let high = (low + 1).min(n - 1);
        let f = source - low as f32;
        let left = tongue.rows[low][0] as f32 * (1.0 - f) + tongue.rows[high][0] as f32 * f;
        let right = tongue.rows[low][1] as f32 * (1.0 - f) + tongue.rows[high][1] as f32 * f;
        let dx = tongue.amp
            * (k as f32 / n as f32).powf(1.5)
            * beat(t, tongue.phase - tongue.lag * k as f32);
        let mut l = round(left + dx);
        let mut r = round(right + dx).max(l);
        if let Some([below_l, below_r]) = previous {
            if l > below_r {
                l = below_r;
                r = below_r;
            }
            if r < below_l {
                l = below_l;
                r = below_l;
            }
        }
        out.push([bottom - k as i32, l, r]);
        previous = Some([l, r]);
    }
    out
}

fn age(t: u64, at: u64) -> u64 {
    (t + CYCLE - at % CYCLE) % CYCLE
}

fn animate(shape: &Shape, grid: &[Vec<Pixel>], t: u64) -> Vec<Vec<Pixel>> {
    let width = grid[0].len() as i32;
    let height = (grid.len() + shape.head + shape.foot) as i32;
    let oy = shape.head as i32;
    let mut mask = vec![vec![false; width as usize]; height as usize];
    let set = |mask: &mut Vec<Vec<bool>>, x: i32, y: i32| {
        if (0..width).contains(&x) && (0..height).contains(&y) {
            mask[y as usize][x as usize] = true;
        }
    };
    for (j, row) in grid.iter().enumerate().skip(shape.body) {
        for (i, pixel) in row.iter().enumerate() {
            if *pixel != Pixel::Empty {
                set(&mut mask, i as i32, j as i32 + oy);
            }
        }
    }
    let bottom = shape.body as i32 - 1 + oy;
    for tongue in shape.tongues {
        for [y, l, r] in tongue_rows(tongue, bottom, t) {
            for x in l..=r {
                set(&mut mask, x, y);
            }
        }
    }
    for bubble in shape.bubbles {
        let age = age(t, bubble.at);
        if age >= bubble.life {
            continue;
        }
        let p = age as f32 / bubble.life as f32;
        let eased = 1.0 - (1.0 - p).powi(2);
        let cx = bubble.from[0] + (bubble.to[0] - bubble.from[0]) * eased;
        let cy = bubble.from[1] + (bubble.to[1] - bubble.from[1]) * eased + oy as f32;
        let radius = bubble.radius[0] + (bubble.radius[1] - bubble.radius[0]) * p;
        for y in (cy - radius).floor() as i32..=(cy + radius).ceil() as i32 {
            for x in (cx - radius).floor() as i32..=(cx + radius).ceil() as i32 {
                if (x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2) <= radius * radius {
                    set(&mut mask, x, y);
                }
            }
        }
    }
    let on = |mask: &Vec<Vec<bool>>, x: i32, y: i32| {
        (0..width).contains(&x) && (0..height).contains(&y) && mask[y as usize][x as usize]
    };
    for _ in 0..2 {
        for y in 0..(shape.body as i32 + oy) {
            for x in 0..width {
                let near = [on(&mask, x + 1, y), on(&mask, x - 1, y), on(&mask, x, y + 1), on(&mask, x, y - 1)];
                let count = near.iter().filter(|lit| **lit).count();
                let here = mask[y as usize][x as usize];
                if !here && count >= 3 {
                    mask[y as usize][x as usize] = true;
                } else if here && count == 1 && (near[0] || near[1]) {
                    mask[y as usize][x as usize] = false;
                }
            }
        }
    }
    let steps = [[1, 0], [-1, 0], [0, 1], [0, -1]];
    let mut distance = vec![vec![0_u32; width as usize]; height as usize];
    let mut queue = VecDeque::new();
    for y in 0..height {
        for x in 0..width {
            if on(&mask, x, y) && steps.iter().any(|[dx, dy]| !on(&mask, x + dx, y + dy)) {
                distance[y as usize][x as usize] = 1;
                queue.push_back([x, y]);
            }
        }
    }
    while let Some([x, y]) = queue.pop_front() {
        for [dx, dy] in steps {
            let (nx, ny) = (x + dx, y + dy);
            if on(&mask, nx, ny) && distance[ny as usize][nx as usize] == 0 {
                distance[ny as usize][nx as usize] = distance[y as usize][x as usize] + 1;
                queue.push_back([nx, ny]);
            }
        }
    }
    let core = grid.iter().flatten().any(|pixel| *pixel == Pixel::Core);
    let ring = |d: u32| {
        if d <= shape.rings[0] {
            Pixel::Edge
        } else if d <= shape.rings[1] || !core {
            Pixel::Mid
        } else {
            Pixel::Core
        }
    };
    let mut out: Vec<Vec<Pixel>> = (0..height)
        .map(|y| {
            (0..width)
                .map(|x| {
                    let source = usize::try_from(y - oy)
                        .ok()
                        .and_then(|row| grid.get(row))
                        .map(|row| row[x as usize]);
                    let below = (y - oy) as usize > shape.body;
                    match source {
                        Some(Pixel::Eye) => Pixel::Eye,
                        Some(pixel @ (Pixel::Edge | Pixel::Mid | Pixel::Core)) if below => pixel,
                        Some(Pixel::Empty | Pixel::Edge | Pixel::Mid | Pixel::Core) | None => {
                            if on(&mask, x, y) {
                                ring(distance[y as usize][x as usize])
                            } else {
                                Pixel::Empty
                            }
                        }
                    }
                })
                .collect()
        })
        .collect();
    for ember in shape.embers {
        let Some(source) = shape.tongues.get(ember.tongue) else {
            continue;
        };
        let age = age(t, ember.at);
        if age >= ember.life {
            continue;
        }
        let Some(&[tip_y, tip_x, _]) = tongue_rows(source, bottom, ember.at).last() else {
            continue;
        };
        let p = age as f32 / ember.life as f32;
        let y = tip_y - 3 - round(ember.rise * (1.0 - (1.0 - p).powi(2)));
        let x = tip_x + round(ember.drift * p);
        let mut put = |x: i32, y: i32, pixel: Pixel| {
            if let Some(cell) = usize::try_from(y)
                .ok()
                .and_then(|row| out.get_mut(row))
                .and_then(|row| usize::try_from(x).ok().and_then(|col| row.get_mut(col)))
                && *cell == Pixel::Empty
            {
                *cell = pixel;
            }
        };
        put(x, y, if p < 0.5 { Pixel::Mid } else { Pixel::Edge });
        if p < 0.3 {
            put(x, y + 1, Pixel::Mid);
        }
    }
    out
}

