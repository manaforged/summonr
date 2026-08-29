const SNIPPET_MAX: usize = 280;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Hit {
    pub(super) title: String,
    pub(super) url: String,
    pub(super) snippet: String,
}

pub(super) fn parse_rss(xml: &str) -> Vec<Hit> {
    let mut hits = Vec::new();
    let mut from = 0;
    while let Some(relative) = xml[from..].find("<item>") {
        let start = from + relative + "<item>".len();
        let Some(relative_end) = xml[start..].find("</item>") else {
            break;
        };
        let end = start + relative_end;
        let item = &xml[start..end];
        let Some(url) = field(item, "link").and_then(result_url) else {
            from = end + "</item>".len();
            continue;
        };
        let title = field(item, "title").unwrap_or_else(|| url.clone());
        if !title.is_empty() {
            hits.push(Hit {
                title: clip(&title, 120),
                url,
                snippet: clip(&field(item, "description").unwrap_or_default(), SNIPPET_MAX),
            });
        }
        from = end + "</item>".len();
    }
    hits
}

pub(super) fn parse_brave(json: &str) -> crate::error::Result<Vec<Hit>> {
    let value: serde_json::Value = serde_json::from_str(json).map_err(|error| {
        crate::error::Error::Tool(format!(
            "web_search returned an unexpected response: {error}"
        ))
    })?;
    let results = value
        .get("web")
        .and_then(|web| web.get("results"))
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok(results
        .iter()
        .filter_map(|item| {
            let text = |key: &str| {
                item.get(key)
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .trim()
                    .to_string()
            };
            let url = text("url");
            (!url.is_empty()).then(|| Hit {
                title: text("title"),
                url,
                snippet: clip(&text("description"), 240),
            })
        })
        .collect())
}

pub(super) fn format_hits(query: &str, hits: &[Hit]) -> String {
    if hits.is_empty() {
        return format!("web_search  0 hits  `{query}`");
    }
    let mut out = format!("web_search  {} hits  `{query}`", hits.len());
    for (index, hit) in hits.iter().enumerate() {
        out.push_str(&format!(
            "\n{}. {}\n   {}\n   {}",
            index + 1,
            hit.title,
            hit.url,
            hit.snippet
        ));
    }
    out.push_str("\nRead chosen URLs with web_fetch.");
    out
}

fn result_url(raw: String) -> Option<String> {
    let url = reqwest::Url::parse(raw.trim()).ok()?;
    matches!(url.scheme(), "http" | "https").then(|| url.to_string())
}

fn field(item: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let start = item.find(&open)? + open.len();
    let end = item[start..].find(&close)? + start;
    Some(plain(&item[start..end]))
}

fn plain(html: &str) -> String {
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
    decode(&out)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn decode(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

pub(super) fn clip(text: &str, max: usize) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() <= max {
        return text;
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}
