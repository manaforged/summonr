use super::space::PLATE;
use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::tui::sigil_view) enum Mark {
    #[default]
    Lines,
    Pixel,
    Banner,
}

impl Mark {
    pub(in crate::tui::sigil_view) fn next(self) -> Self {
        match self {
            Self::Lines => Self::Pixel,
            Self::Pixel => Self::Banner,
            Self::Banner => Self::Lines,
        }
    }
}

const LAYERS: i32 = 5;
const STEP_MS: u64 = 90;

#[derive(Clone, Copy)]
struct Glyph {
    text: char,
    fg: Rgba8,
    bg: Option<Rgba8>,
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
    let grid = draw(mark, u64::MAX / 2);
    [grid.first().map_or(0, Vec::len) as u16, grid.len() as u16]
}

pub(super) fn paint(cells: &mut [Cell], cols: u16, x: u16, y: u16, mark: Mark, elapsed: u64) {
    for (row, line) in draw(mark, elapsed).iter().enumerate() {
        for (col, glyph) in line.iter().enumerate() {
            let Some(glyph) = glyph else { continue };
            let index =
                usize::from(y + row as u16) * usize::from(cols) + usize::from(x + col as u16);
            if let Some(cell) = cells.get_mut(index) {
                let mut buffer = [0; 4];
                cell.text = (&*glyph.text.encode_utf8(&mut buffer)).into();
                cell.fg = ink(glyph.fg);
                if let Some(bg) = glyph.bg {
                    cell.bg = ink(bg);
                }
            }
        }
    }
}

fn draw(mark: Mark, elapsed: u64) -> Grid {
    let built = i32::try_from(elapsed / STEP_MS).unwrap_or(i32::MAX);
    match mark {
        Mark::Lines => stack(built),
        Mark::Pixel => pixels(built),
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

fn stack(built: i32) -> Grid {
    let (width, vert) = (14, 3);
    let y = bars(2, 1);
    let outline = [
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
    ];
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
    }
    grid
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
    let glyph = |text| Some(Glyph { text, fg, bg: None });
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

fn pixels(built: i32) -> Grid {
    let (width, vert) = (20, 4);
    let y = bars(4, 3);
    let on = |x: i32, row: i32| {
        (y[0]..y[1]).contains(&row)
            || (y[1]..y[2]).contains(&row) && x < vert
            || (y[2]..y[3]).contains(&row)
            || (y[3]..y[4]).contains(&row) && x >= width - vert
            || (y[4]..y[5]).contains(&row)
    };
    let tall = y[5] + LAYERS;
    let mut dots = vec![vec![None; (width + LAYERS) as usize]; tall as usize];
    let mut lay = |shift: (i32, i32), fg: Rgba8| {
        for row in 0..y[5] {
            for x in (0..width).filter(|&x| on(x, row)) {
                dots[(row + shift.1) as usize][(x + shift.0) as usize] = Some(fg);
            }
        }
    };
    for layer in (1..=LAYERS).rev().filter(|layer| LAYERS - layer < built) {
        lay((layer, LAYERS - layer), depth(layer));
    }
    if built > LAYERS {
        lay((0, LAYERS), palette().text);
    }
    (0..(tall + 1) / 2)
        .map(|row| {
            (0..width + LAYERS)
                .map(|x| {
                    let pick = |dy: i32| {
                        dots.get((2 * row + dy) as usize)
                            .and_then(|line| line[x as usize])
                    };
                    match (pick(0), pick(1)) {
                        (Some(top), Some(bottom)) if top == bottom => Some(Glyph {
                            text: '█',
                            fg: top,
                            bg: None,
                        }),
                        (Some(top), Some(bottom)) => Some(Glyph {
                            text: '▀',
                            fg: top,
                            bg: Some(bottom),
                        }),
                        (Some(top), None) => Some(Glyph {
                            text: '▀',
                            fg: top,
                            bg: None,
                        }),
                        (None, Some(bottom)) => Some(Glyph {
                            text: '▄',
                            fg: bottom,
                            bg: None,
                        }),
                        (None, None) => None,
                    }
                })
                .collect()
        })
        .collect()
}
