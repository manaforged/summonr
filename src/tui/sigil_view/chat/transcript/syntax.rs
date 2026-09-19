use super::*;

pub(super) fn literal(text: &str, offset: usize) -> impl Iterator<Item = Line> + '_ {
    text.split_inclusive('\n').scan(offset, |byte, raw| {
        let mut offset = *byte;
        *byte += raw.len();
        let mut spans = code(raw.trim_end_matches(['\r', '\n']));
        for span in spans.iter_mut().skip(1) {
            span.source = Some(Source::Text(offset..offset + span.text.len()));
            offset += span.text.len();
        }
        Some(Line {
            spans,
            bg: palette().code_bg,
        })
    })
}

pub(super) fn diagram_fence(info: &str) -> bool {
    let info = info.trim();
    info.starts_with("mermaid") || info.eq_ignore_ascii_case("chart")
}

pub(super) fn diagram(source: &str, width: usize, range: Range<usize>) -> Vec<Line> {
    let drawing =
        super::chart::bars(source).unwrap_or_else(|| sigil_tui::widgets::mermaid_draw(source));
    if drawing.iter().any(|line| cell_width(line) + 2 > width) {
        let mut lines = vec![Line {
            spans: vec![Span::surface(
                "mermaid source · widen to view diagram",
                palette().dim,
                palette().code_bg,
            )],
            bg: palette().code_bg,
        }];
        lines.extend(literal(source, range.start));
        return lines;
    }
    drawing
        .into_iter()
        .map(|line| {
            let mut span = Span::surface(format!("  {line}"), palette().text, palette().code_bg);
            span.source = Some(Source::Block(range.clone()));
            Line {
                spans: vec![span],
                bg: palette().code_bg,
            }
        })
        .collect()
}

pub(crate) fn numbered(line: &str) -> Option<(&str, &str)> {
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 || digits > 3 {
        return None;
    }
    let rest = line[digits..].strip_prefix(". ")?;
    Some((&line[..digits], rest))
}

pub(super) fn inline(text: &str, accent: Rgba8, offset: usize) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut plain = String::new();
    let mut chars = text.char_indices().peekable();
    let mut start = 0;
    let mut bold = false;
    let mut code = false;
    while let Some((index, ch)) = chars.next() {
        let marker = (ch == '*' && chars.peek().is_some_and(|(_, ch)| *ch == '*')) || ch == '`';
        if marker {
            if ch == '*' {
                chars.next();
            }
            if !plain.is_empty() {
                let mut span = Span::styled(
                    std::mem::take(&mut plain),
                    if code { accent } else { palette().text },
                );
                span.bold = bold;
                span.source = Some(Source::Text(offset + start..offset + index));
                spans.push(span);
            }
            start = index + if ch == '*' { 2 } else { 1 };
            if ch == '*' {
                bold = !bold;
            } else {
                code = !code;
            }
        } else {
            plain.push(ch);
        }
    }
    if !plain.is_empty() || spans.is_empty() {
        let mut span = Span::styled(plain, if code { accent } else { palette().text });
        span.bold = bold;
        span.source = Some(Source::Text(offset + start..offset + text.len()));
        spans.push(span);
    }
    spans
}

pub(super) const KEYWORDS: [&str; 44] = [
    "as",
    "async",
    "await",
    "break",
    "class",
    "const",
    "continue",
    "def",
    "dyn",
    "else",
    "enum",
    "export",
    "false",
    "fn",
    "for",
    "from",
    "function",
    "if",
    "impl",
    "import",
    "in",
    "interface",
    "let",
    "loop",
    "match",
    "mod",
    "mut",
    "new",
    "null",
    "pub",
    "return",
    "self",
    "static",
    "struct",
    "trait",
    "true",
    "type",
    "unsafe",
    "use",
    "var",
    "where",
    "while",
    "None",
    "Some",
];

pub(crate) fn code(raw: &str) -> Vec<Span> {
    let mut spans = vec![Span::surface("  ", palette().text, palette().code_bg)];
    let mut text = raw;
    if let Some(index) = raw.find("//") {
        let (plain, comment) = raw.split_at(index);
        words(&mut spans, plain);
        spans.push(Span::surface(
            comment,
            palette().code_comment,
            palette().code_bg,
        ));
        return spans;
    }
    while let Some(start) = text.find(['\"', '\'']) {
        let quote = text.as_bytes()[start] as char;
        let after = &text[start + 1..];
        let Some(end) = after.find(quote) else {
            break;
        };
        words(&mut spans, &text[..start]);
        spans.push(Span::surface(
            &text[start..start + end + 2],
            palette().code_string,
            palette().code_bg,
        ));
        text = &text[start + end + 2..];
    }
    words(&mut spans, text);
    spans
}

pub(super) fn words(spans: &mut Vec<Span>, text: &str) {
    let mut rest = text;
    while !rest.is_empty() {
        let word = rest
            .find(|c: char| !(c.is_alphanumeric() || c == '_'))
            .unwrap_or(rest.len());
        if word == 0 {
            let gap = rest
                .find(|c: char| c.is_alphanumeric() || c == '_')
                .unwrap_or(rest.len());
            spans.push(Span::surface(
                &rest[..gap],
                palette().text,
                palette().code_bg,
            ));
            rest = &rest[gap..];
            continue;
        }
        let color = if KEYWORDS.contains(&&rest[..word]) {
            palette().code_keyword
        } else {
            palette().text
        };
        spans.push(Span::surface(&rest[..word], color, palette().code_bg));
        rest = &rest[word..];
    }
}
