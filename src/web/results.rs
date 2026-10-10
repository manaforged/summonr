use std::collections::HashMap;
use std::sync::OnceLock;

use serde::Deserialize;
use url::Url;

use super::{domain_match, host_of};
use crate::error::{Error, Result};
use crate::text::ellipsize;

const SNIPPET_MAX: usize = 280;
const TITLE_MAX: usize = 120;
const ENGINES: &str = include_str!("engines.toml");

#[derive(Deserialize)]
struct Table {
    engine: Vec<Engine>,
}

#[derive(Debug, Deserialize)]
pub(super) struct Engine {
    pub(super) name: String,
    url: String,
    query: String,
    block: String,
    link: String,
    #[serde(default)]
    title: Option<String>,
    snippet: String,
    #[serde(default)]
    unwrap: Option<String>,
    #[serde(default)]
    freshness: Option<Freshness>,
    #[serde(default)]
    hosts: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum Window {
    Day,
    Week,
    Month,
}

impl Window {
    pub(super) fn parse(value: Option<&str>) -> Result<Option<Self>> {
        let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
            return Ok(None);
        };
        let reader = serde::de::value::StrDeserializer::<serde::de::value::Error>::new(value);
        Self::deserialize(reader).map(Some).map_err(|error| {
            Error::Tool(format!(
                "web_search freshness takes day, week, or month, not `{value}` ({error})"
            ))
        })
    }
}

#[derive(Debug, Deserialize)]
struct Freshness {
    param: String,
    values: HashMap<Window, String>,
}

pub(super) fn table(text: &str) -> std::result::Result<Vec<Engine>, String> {
    let table: Table = toml::from_str(text).map_err(|error| error.to_string())?;
    Ok(table.engine)
}

pub(super) fn engines() -> Result<&'static [Engine]> {
    static ROWS: OnceLock<std::result::Result<Vec<Engine>, String>> = OnceLock::new();
    ROWS.get_or_init(|| table(ENGINES))
        .as_deref()
        .map_err(|error| Error::Tool(format!("web_search engines: {error}")))
}

impl Engine {
    pub(super) fn request(&self, query: &str, freshness: Option<Window>) -> Result<Url> {
        let mut url = Url::parse(&self.url)
            .map_err(|error| Error::Tool(format!("web_search url: {error}")))?;
        {
            let mut pairs = url.query_pairs_mut();
            pairs.append_pair(&self.query, query);
            if let Some((param, code)) =
                self.freshness
                    .as_ref()
                    .zip(freshness)
                    .and_then(|(table, window)| {
                        Some((table.param.as_str(), table.values.get(&window)?.as_str()))
                    })
            {
                pairs.append_pair(param, code);
            }
        }
        Ok(url)
    }

    fn target(&self, href: &str) -> Option<String> {
        let absolute = result_url(href.to_string())?;
        let Some(param) = &self.unwrap else {
            return Some(absolute);
        };
        let inner = Url::parse(&absolute)
            .ok()?
            .query_pairs()
            .find(|(key, _)| key == param)
            .map(|(_, value)| value.into_owned());
        match inner {
            Some(inner) => result_url(inner),
            None => Some(absolute),
        }
    }

    fn own(&self, url: &str) -> bool {
        let host = host_of(url);
        self.hosts.iter().any(|own| domain_match(&host, own))
    }
}

pub(super) fn parse_page(html: &str, engine: &Engine) -> Vec<Hit> {
    html.split(engine.block.as_str())
        .skip(1)
        .filter_map(|block| page_hit(block, engine))
        .collect()
}

fn page_hit(block: &str, engine: &Engine) -> Option<Hit> {
    let anchor = element(block, &engine.link)?;
    let href = attr(&anchor[..anchor.find('>')? + 1], "href")?;
    let url = engine.target(href)?;
    if engine.own(&url) {
        return None;
    }
    let title = engine
        .title
        .as_deref()
        .and_then(|class| element(anchor, class))
        .map_or_else(|| plain(anchor), plain);
    if title.is_empty() {
        return None;
    }
    let snippet = element(block, &engine.snippet)
        .map(plain)
        .unwrap_or_default();
    Some(Hit {
        title: ellipsize(&title, TITLE_MAX),
        url,
        snippet: ellipsize(&snippet, SNIPPET_MAX),
    })
}

fn element<'a>(html: &'a str, class: &str) -> Option<&'a str> {
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

fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let marker = format!(" {name}=\"");
    let start = tag.find(&marker)? + marker.len();
    let end = start + tag[start..].find('"')?;
    Some(&tag[start..end])
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Hit {
    pub(super) title: String,
    pub(super) url: String,
    pub(super) snippet: String,
}

fn result_url(raw: String) -> Option<String> {
    let raw = decode(raw.trim());
    let raw = raw
        .strip_prefix("//")
        .map_or(raw.clone(), |rest| format!("https://{rest}"));
    let url = Url::parse(&raw).ok()?;
    matches!(url.scheme(), "http" | "https").then(|| url.to_string())
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

pub(super) fn flat(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
