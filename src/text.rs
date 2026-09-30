use sigil::app::cell_width;
use unicode_segmentation::UnicodeSegmentation;

pub(crate) fn ellipsize(text: &str, width: usize) -> String {
    if cell_width(text) <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let mut used = 0;
    let end = text.grapheme_indices(true).find_map(|(index, grapheme)| {
        used += cell_width(grapheme);
        (used > width.saturating_sub(1)).then_some(index)
    });
    let mut shown = text[..end.unwrap_or(text.len())].to_string();
    shown.push('…');
    shown
}

pub(crate) fn ellipsize_start(text: &str, width: usize) -> String {
    if cell_width(text) <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let mut used = 0;
    let start = text
        .grapheme_indices(true)
        .rev()
        .find_map(|(index, grapheme)| {
            used += cell_width(grapheme);
            (used > width.saturating_sub(1)).then_some(index + grapheme.len())
        });
    format!("…{}", &text[start.unwrap_or(0)..])
}

pub(crate) fn fnv64(bytes: &[u8]) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

pub(crate) fn redact<'a>(text: &str, secrets: impl IntoIterator<Item = &'a str>) -> String {
    secrets
        .into_iter()
        .filter(|secret| !secret.is_empty())
        .fold(text.to_string(), |text, secret| {
            text.replace(secret, "[secret]")
        })
}
