use std::collections::HashMap;
use std::sync::OnceLock;

use serde::Deserialize;
use url::Url;

use super::{domain_match, host_of};
use crate::error::{Error, Result};
use crate::text::ellipsize;

mod markup;

use markup::{attr, decode, element, plain, tag};

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
    link: Option<String>,
    link_in: Option<Container>,
    title: Option<String>,
    snippet: String,
    unwrap: Option<String>,
    #[serde(default)]
    carrier: Carrier,
    unwrap_prefix: Option<String>,
    #[serde(default)]
    encoding: Encoding,
    freshness: Option<Freshness>,
    #[serde(default)]
    hosts: Vec<String>,
    block_status: Vec<u16>,
    #[serde(default)]
    block_markers: Vec<String>,
    cooldown_secs: u64,
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
#[serde(rename_all = "lowercase")]
enum Container {
    Tag(String),
    Class(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Carrier {
    #[default]
    Query,
    Path,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Encoding {
    #[default]
    Url,
    Base64,
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
        let parsed = Url::parse(&absolute).ok()?;
        let inner = match self.carrier {
            Carrier::Query => parsed
                .query_pairs()
                .find(|(key, _)| key == param)
                .map(|(_, value)| value.into_owned()),
            Carrier::Path => parsed.path_segments().and_then(|mut segments| {
                let value = segments.find_map(|segment| {
                    segment
                        .strip_prefix(param.as_str())
                        .and_then(|rest| rest.strip_prefix('='))
                })?;
                url::form_urlencoded::parse(format!("v={value}").as_bytes())
                    .next()
                    .map(|(_, decoded)| decoded.into_owned())
            }),
        };
        let Some(inner) = inner else {
            return Some(absolute);
        };
        let inner = match &self.unwrap_prefix {
            Some(prefix) => inner.strip_prefix(prefix.as_str())?.to_string(),
            None => inner,
        };
        match self.encoding {
            Encoding::Url => result_url(inner),
            Encoding::Base64 => {
                use base64::Engine as _;
                let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
                    .decode(inner.trim_end_matches('='))
                    .ok()?;
                result_url(String::from_utf8(bytes).ok()?)
            }
        }
    }

    fn anchor<'a>(&self, block: &'a str) -> Option<&'a str> {
        match (&self.link_in, &self.link) {
            (Some(Container::Tag(name)), _) => tag(tag(block, name)?, "a"),
            (Some(Container::Class(class)), _) => tag(element(block, class)?, "a"),
            (None, Some(link)) => element(block, link),
            (None, None) => None,
        }
    }

    pub(super) fn blocked(&self, status: u16, body: &str) -> bool {
        self.block_status.contains(&status)
            || self
                .block_markers
                .iter()
                .any(|marker| body.contains(marker.as_str()))
    }

    pub(super) fn cooldown(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.cooldown_secs)
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
    let anchor = engine.anchor(block)?;
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Hit {
    pub(super) title: String,
    pub(super) url: String,
    pub(super) snippet: String,
}

fn result_url(raw: String) -> Option<String> {
    let raw = decode(raw.trim());
    let raw = match raw.strip_prefix("//") {
        Some(rest) => format!("https://{rest}"),
        None => raw,
    };
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

pub(super) fn flat(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
