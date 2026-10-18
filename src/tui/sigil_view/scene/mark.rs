use super::space::{PLATE, Plate};
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::tui::sigil_view) enum Mark {
    Lines,
    Banner,
}

pub(super) const LAYERS: i32 = 5;
const STEP_MS: u64 = 90;
const LAP_MS: u64 = 8_000;
const TAIL_PERCENT: usize = 10;
const SHIMMER: f32 = 0.35;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Glyph {
    text: char,
    fg: Rgba8,
}

struct Lines {
    straight: [char; 2],
    corners: [char; 4],
}

const LIGHT: Lines = Lines {
    straight: ['─', '│'],
    corners: ['┌', '┐', '└', '┘'],
};
const HEAVY: Lines = Lines {
    straight: ['━', '┃'],
    corners: ['┏', '┓', '┗', '┛'],
};

type Grid = Vec<Vec<Option<Glyph>>>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Left,
    Right,
    Up,
    Down,
}

impl Side {
    fn from_step(x: i32, y: i32) -> Self {
        match (x, y) {
            (1, _) => Self::Right,
            (-1, _) => Self::Left,
            (_, 1) => Self::Down,
            _ => Self::Up,
        }
    }
}

pub(super) fn size(mark: Mark) -> [u16; 2] {
    if mark == Mark::Banner {
        return [
            crate::splash::logo_width() as u16,
            crate::splash::logo().len() as u16,
        ];
    }
    let grid = draw(mark, u64::MAX / 2, None);
    [grid.first().map_or(0, Vec::len) as u16, grid.len() as u16]
}

#[derive(Clone, Debug, PartialEq)]
pub(in crate::tui::sigil_view) struct Shimmer {
    at: [u16; 2],
    grid: Grid,
    path: Vec<(i32, i32)>,
}

impl Shimmer {
    pub(super) fn apply(&self, plate: &mut Plate, clock: u64, lit: &mut Vec<[u16; 2]>) {
        for (x, y, strength) in trail(&self.path, clock) {
            let (col, row) = (x, LAYERS + y);
            let Some(Some(glyph)) = usize::try_from(row)
                .ok()
                .and_then(|row| self.grid.get(row))
                .zip(usize::try_from(col).ok())
                .and_then(|(line, col)| line.get(col))
            else {
                continue;
            };
            let at = [self.at[0] + col as u16, self.at[1] + row as u16];
            if let Some(cell) = plate.cell(at) {
                cell.fg = ink(blend(glyph.fg, palette().accent, strength));
                lit.push(at);
            }
        }
    }
}

pub(super) fn paint(
    plate: &mut Plate,
    [x, y]: [u16; 2],
    mark: Mark,
    elapsed: u64,
) -> Option<Shimmer> {
    let grid = draw(mark, elapsed, None);
    for (row, line) in grid.iter().enumerate() {
        for (col, glyph) in line.iter().enumerate() {
            let Some(glyph) = glyph else { continue };
            if let Some(cell) = plate.cell([x + col as u16, y + row as u16]) {
                let mut buffer = [0; 4];
                cell.text = (&*glyph.text.encode_utf8(&mut buffer)).into();
                cell.fg = ink(glyph.fg);
            }
        }
    }
    (mark == Mark::Lines && built(elapsed) > LAYERS).then(|| Shimmer {
        at: [x, y],
        grid,
        path: perimeter(&outline()),
    })
}

fn built(elapsed: u64) -> i32 {
    i32::try_from(elapsed / STEP_MS).unwrap_or(i32::MAX)
}

fn draw(mark: Mark, elapsed: u64, clock: Option<u64>) -> Grid {
    match mark {
        Mark::Lines => stack(built(elapsed), clock),
        Mark::Banner => Vec::new(),
    }
}

fn depth(layer: i32) -> Rgba8 {
    let front = (LAYERS - layer) as f32 / (LAYERS - 1) as f32;
    blend(PLATE, palette().text, 0.14 + front * 0.34)
}

fn bars(bar: i32, gap: i32) -> [i32; 6] {
    [
        0,
        bar,
        bar + gap,
        2 * bar + gap,
        2 * bar + 2 * gap,
        3 * bar + 2 * gap,
    ]
}

const WIDTH: i32 = 27;

fn outline() -> [(i32, i32); 12] {
    let (width, vert) = (WIDTH, 5);
    let y = bars(3, 2);
    [
        (0, y[0]),
        (width, y[0]),
        (width, y[1]),
        (vert, y[1]),
        (vert, y[2]),
        (width, y[2]),
        (width, y[5]),
        (0, y[5]),
        (0, y[4]),
        (width - vert, y[4]),
        (width - vert, y[3]),
        (0, y[3]),
    ]
}

fn stack(built: i32, clock: Option<u64>) -> Grid {
    let (width, y) = (WIDTH, bars(3, 2));
    let outline = outline();
    let mut grid: Grid =
        vec![vec![None; (width + 1 + LAYERS * 2) as usize]; (y[5] + 1 + LAYERS) as usize];
    for layer in (1..=LAYERS).rev().filter(|layer| LAYERS - layer < built) {
        stroke(
            &mut grid,
            &outline,
            (layer * 2, LAYERS - layer),
            &LIGHT,
            depth(layer),
        );
    }
    if built > LAYERS {
        stroke(&mut grid, &outline, (0, LAYERS), &HEAVY, palette().text);
        if let Some(clock) = clock {
            orbit(&mut grid, &outline, (0, LAYERS), clock);
        }
    }
    grid
}

fn orbit(grid: &mut Grid, outline: &[(i32, i32)], (ox, oy): (i32, i32), clock: u64) {
    for (x, y, strength) in trail(&perimeter(outline), clock) {
        if let Some(Some(glyph)) = usize::try_from(oy + y)
            .ok()
            .and_then(|row| grid.get_mut(row))
            .zip(usize::try_from(ox + x).ok())
            .and_then(|(row, col)| row.get_mut(col))
        {
            glyph.fg = blend(glyph.fg, palette().accent, strength);
        }
    }
}

fn trail(path: &[(i32, i32)], clock: u64) -> impl Iterator<Item = (i32, i32, f32)> + '_ {
    let cells = path.len();
    let (head, tail) = if cells == 0 {
        (0, 0)
    } else {
        (
            (clock % LAP_MS) as usize * cells / LAP_MS as usize,
            (cells * TAIL_PERCENT / 100).max(3),
        )
    };
    (0..tail).map(move |back| {
        let (x, y) = path[(head + cells - back) % cells];
        (x, y, SHIMMER * (1.0 - back as f32 / tail as f32))
    })
}

fn perimeter(outline: &[(i32, i32)]) -> Vec<(i32, i32)> {
    (0..outline.len())
        .flat_map(|at| {
            let (x0, y0) = outline[at];
            let (x1, y1) = outline[(at + 1) % outline.len()];
            let steps = (x1 - x0).abs().max((y1 - y0).abs());
            (0..steps).map(move |step| {
                (
                    x0 + (x1 - x0).signum() * step,
                    y0 + (y1 - y0).signum() * step,
                )
            })
        })
        .collect()
}

fn inside(outline: &[(i32, i32)], x: f32, y: f32) -> bool {
    let mut hit = false;
    for (at, &(xi, yi)) in outline.iter().enumerate() {
        let (xj, yj) = outline[(at + outline.len() - 1) % outline.len()];
        let (xi, yi, xj, yj) = (xi as f32, yi as f32, xj as f32, yj as f32);
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            hit = !hit;
        }
    }
    hit
}

fn put(grid: &mut Grid, x: i32, y: i32, glyph: Option<Glyph>) {
    if let Some(cell) = usize::try_from(y)
        .ok()
        .and_then(|y| grid.get_mut(y))
        .zip(usize::try_from(x).ok())
        .and_then(|(row, x)| row.get_mut(x))
    {
        *cell = glyph;
    }
}

fn stroke(grid: &mut Grid, outline: &[(i32, i32)], (ox, oy): (i32, i32), lines: &Lines, fg: Rgba8) {
    let tall = outline.iter().map(|point| point.1).max().unwrap_or(0);
    let wide = outline.iter().map(|point| point.0).max().unwrap_or(0);
    for y in 0..=tall {
        for x in 0..=wide {
            if inside(outline, x as f32 + 0.5, y as f32 + 0.5)
                || inside(outline, x as f32, y as f32)
            {
                put(grid, ox + x, oy + y, None);
            }
        }
    }
    let glyph = |text| Some(Glyph { text, fg });
    for at in 0..outline.len() {
        let (px, py) = outline[(at + outline.len() - 1) % outline.len()];
        let (x0, y0) = outline[at];
        let (x1, y1) = outline[(at + 1) % outline.len()];
        let steps = (x1 - x0).abs().max((y1 - y0).abs());
        for step in 1..steps {
            let (x, y) = (
                x0 + (x1 - x0).signum() * step,
                y0 + (y1 - y0).signum() * step,
            );
            put(
                grid,
                ox + x,
                oy + y,
                glyph(lines.straight[usize::from(y1 != y0)]),
            );
        }
        let [from, to] = [
            Side::from_step((px - x0).signum(), (py - y0).signum()),
            Side::from_step((x1 - x0).signum(), (y1 - y0).signum()),
        ];
        let down = from == Side::Down || to == Side::Down;
        let right = from == Side::Right || to == Side::Right;
        let corner = match (down, right) {
            (true, true) => 0,
            (true, false) => 1,
            (false, true) => 2,
            (false, false) => 3,
        };
        put(grid, ox + x0, oy + y0, glyph(lines.corners[corner]));
    }
}

#[cfg(test)]
#[path = "mark_tests.rs"]
mod tests;
