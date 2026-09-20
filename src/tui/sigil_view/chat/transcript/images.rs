use super::*;

pub(super) fn markdown_image(line: &str) -> Option<(&str, &str)> {
    let rest = line.trim().strip_prefix("![")?;
    let mid = rest.find("](")?;
    let alt = &rest[..mid];
    let url = rest[mid + 2..].strip_suffix(')')?;
    let url = url
        .trim()
        .trim_matches(|c| c == '<' || c == '>' || c == '"');
    Some((alt, url))
}

pub(super) fn decode_markdown_image(url: &str) -> Option<ImageSource> {
    if let Some(data) = url.strip_prefix("data:") {
        let (meta, payload) = data.split_once(',')?;
        if !meta.contains("base64") || meta.contains("svg") {
            return None;
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(payload)
            .ok()?;
        return super::model::preview(&bytes).ok();
    }
    if url.contains("://") {
        return None;
    }
    let path = std::path::Path::new(url);
    let bytes = std::fs::read(path).ok()?;
    super::model::preview(&bytes).ok()
}

pub(crate) fn image_cells(source: &ImageSource, width: usize) -> (usize, usize) {
    let pixel_width = source.width() as usize;
    let pixel_height = source.height() as usize;
    if width == 0 || pixel_width == 0 || pixel_height == 0 {
        return (0, 0);
    }
    let aspect = super::canvas::cell_aspect();
    let cols = if aspect <= 1.0 {
        width.max(1)
    } else {
        width.min(pixel_width).max(1)
    };
    let rows = ((pixel_height as f32 * cols as f32) / (pixel_width as f32 * aspect))
        .ceil()
        .clamp(1.0, ISUMMONR_ROWS as f32) as usize;
    let cols = ((rows as f32 * aspect * pixel_width as f32) / pixel_height as f32)
        .round()
        .clamp(1.0, width as f32) as usize;
    (cols.max(1), rows.max(1))
}

pub(crate) fn fit_source(source: &ImageSource, cols: usize, rows: usize) -> ImageSource {
    if super::canvas::cell_aspect() <= 1.0
        || cols == 0
        || rows == 0
        || sigil_tui::protocol().is_some()
    {
        return source.clone();
    }
    let width = u32::try_from(cols).unwrap_or(u32::MAX);
    let height = u32::try_from(rows.saturating_mul(2)).unwrap_or(u32::MAX);
    let key = (source.id(), width, height);
    if let Some(hit) = CACHE.with(|cache| cache.borrow().get(&key).cloned()) {
        return hit;
    }
    let resized = resized(source, width, height);
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.len() >= CACHE_MAX {
            cache.clear();
        }
        cache.insert(key, resized.clone());
    });
    resized
}

pub(super) fn resized(source: &ImageSource, width: u32, height: u32) -> ImageSource {
    ::image::RgbaImage::from_raw(source.width(), source.height(), source.rgba().to_vec())
        .map(|buffer| {
            ::image::imageops::resize(
                &buffer,
                width,
                height,
                ::image::imageops::FilterType::Lanczos3,
            )
        })
        .and_then(|buffer| ImageSource::from_rgba8(width, height, buffer.into_raw()))
        .unwrap_or_else(|| source.clone())
}

pub(super) const ISUMMONR_ROWS: usize = 48;
