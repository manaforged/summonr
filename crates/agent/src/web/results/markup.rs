pub(in crate::web) fn element<'a>(html: &'a str, class: &str) -> Option<&'a str> {
    let marker = "class=\"";
    let mut from = 0;
    while let Some(relative) = html[from..].find(marker) {
        let start = from + relative + marker.len();
        let end = start + html[start..].find('"')?;
        if html[start..end]
            .split_whitespace()
            .any(|token| token == class)
        {
            let open = html[..start].rfind('<')?;
            return Some(&html[open..closing(html, open)?]);
        }
        from = end;
    }
    None
}

pub(in crate::web) fn tag<'a>(html: &'a str, name: &str) -> Option<&'a str> {
    let opener = format!("<{name}");
    let mut from = 0;
    while let Some(relative) = html[from..].find(&opener) {
        let open = from + relative;
        let after = open + opener.len();
        if html[after..].starts_with(|c: char| c.is_whitespace() || c == '>') {
            return Some(&html[open..closing(html, open)?]);
        }
        from = after;
    }
    None
}

fn closing(html: &str, open: usize) -> Option<usize> {
    let rest = &html[open + 1..];
    let name = &rest[..rest.find(|c: char| c.is_whitespace() || c == '>' || c == '/')?];
    let opener = format!("<{name}");
    let closer = format!("</{name}>");
    let mut depth = 0usize;
    let mut at = open;
    loop {
        let close = at + html[at..].find(&closer)?;
        let next = html[at..close].find(&opener).map(|index| at + index);
        match next {
            Some(index) => {
                at = index + opener.len();
                if html[at..].starts_with(|c: char| c.is_whitespace() || c == '>' || c == '/') {
                    depth += 1;
                }
            }
            None => {
                at = close + closer.len();
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(at);
                }
            }
        }
    }
}

pub(in crate::web) fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!("{name}=");
    let (at, _) = tag
        .to_ascii_lowercase()
        .match_indices(&needle)
        .find(|(at, _)| tag[..*at].ends_with(char::is_whitespace))?;
    let rest = tag[at + needle.len()..].trim_start();
    match rest.chars().next()? {
        quote @ ('"' | '\'') => rest[1..].find(quote).map(|end| &rest[1..1 + end]),
        _ => rest
            .split(|c: char| c.is_whitespace() || c == '>')
            .next()
            .map(|value| value.trim_end_matches('/')),
    }
}

pub(in crate::web) fn strip(html: &str) -> String {
    let mut out = String::new();
    let mut tag = false;
    for ch in html.chars() {
        match ch {
            '<' => tag = true,
            '>' => tag = false,
            _ if !tag => out.push(ch),
            _ => {}
        }
    }
    out
}

pub(in crate::web) fn text(html: &str) -> String {
    decode(&strip(html))
}

#[derive(Clone, Copy)]
pub(in crate::web) enum Space {
    Flat,
    Lines,
}

pub(in crate::web) fn collapse(text: &str, space: Space) -> String {
    if let Space::Flat = space {
        return text.split_whitespace().collect::<Vec<_>>().join(" ");
    }
    let mut out = String::new();
    let mut blank = 0;
    for line in text.lines() {
        let line = collapse(line, Space::Flat);
        if line.is_empty() {
            blank += 1;
            if blank <= 1 && !out.is_empty() {
                out.push('\n');
            }
        } else {
            blank = 0;
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(&line);
        }
    }
    out
}

pub(in crate::web) fn decode(text: &str) -> String {
    numeric(text)
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .replace("&middot;", "·")
        .replace("&hellip;", "…")
        .replace("&ndash;", "–")
        .replace("&mdash;", "—")
}

fn numeric(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("&#") {
        out.push_str(&rest[..at]);
        let body = &rest[at + 2..];
        let decoded = body.find(';').and_then(|end| {
            let code = &body[..end];
            let value = match code.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => code.parse().ok()?,
            };
            Some((char::from_u32(value)?, end))
        });
        match decoded {
            Some((ch, end)) => {
                out.push(ch);
                rest = &body[end + 1..];
            }
            None => {
                out.push_str("&#");
                rest = body;
            }
        }
    }
    out.push_str(rest);
    out
}
