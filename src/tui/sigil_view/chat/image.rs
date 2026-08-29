use std::cell::RefCell;
use std::collections::HashMap;

use sigil::prelude::Rgba8;

use super::{BASE, Line, Span};

const MAX_ROWS: usize = 24;

thread_local! {
    static CACHE: RefCell<HashMap<(u64, usize), Vec<Line>>> = RefCell::new(HashMap::new());
}

pub(super) fn render_cached(bytes: &[u8], width: usize) -> Result<Vec<Line>, String> {
    let key = (identity(bytes), width);
    if let Some(hit) = CACHE.with(|cache| cache.borrow().get(&key).cloned()) {
        return Ok(hit);
    }
    let rows = render(bytes, width)?;
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.len() >= 32 {
            cache.clear();
        }
        cache.insert(key, rows.clone());
    });
    Ok(rows)
}

fn identity(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64 ^ bytes.len() as u64;
    for byte in bytes.iter().take(512) {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

pub(super) fn render(bytes: &[u8], width: usize) -> Result<Vec<Line>, String> {
    if width == 0 {
        return Ok(Vec::new());
    }
    let image = image::load_from_memory(bytes).map_err(|error| error.to_string())?;
    let rgba = image.to_rgba8();
    let (source_width, source_height) = rgba.dimensions();
    if source_width == 0 || source_height == 0 {
        return Err("image has no pixels".to_string());
    }
    let source_width = usize::try_from(source_width).unwrap_or(usize::MAX);
    let source_height = usize::try_from(source_height).unwrap_or(usize::MAX);

    let cell_width = width.min(source_width).max(1);
    let pixel_height = (source_height * cell_width) / source_width;
    let cell_height = pixel_height.div_ceil(2).min(MAX_ROWS);

    let mut rows = Vec::with_capacity(cell_height);
    for row in 0..cell_height {
        let mut line = Vec::with_capacity(cell_width);
        for column in 0..cell_width {
            let x = column * source_width / cell_width;
            let y_top = (row * 2 * source_height) / (cell_height * 2);
            let y_bottom = ((row * 2 + 1) * source_height) / (cell_height * 2);
            let top = rgba.get_pixel(x as u32, y_top as u32).0;
            let bottom = if row * 2 + 1 < pixel_height {
                Some(rgba.get_pixel(x as u32, y_bottom as u32).0)
            } else {
                None
            };
            line.push(Span::surface(
                "▀",
                composite(top),
                bottom.map_or(BASE, composite),
            ));
        }
        rows.push(line);
    }
    Ok(rows)
}

fn composite(pixel: [u8; 4]) -> Rgba8 {
    let [red, green, blue, alpha] = pixel;
    let alpha = f32::from(alpha) / 255.0;
    let channel = |channel: u8, base: u8| {
        (f32::from(channel) * alpha + f32::from(base) * (1.0 - alpha)).round() as u8
    };
    Rgba8 {
        r: channel(red, BASE.r),
        g: channel(green, BASE.g),
        b: channel(blue, BASE.b),
        a: 255,
    }
}

#[cfg(test)]
#[path = "image_tests.rs"]
mod tests;
