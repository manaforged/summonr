use std::net::ToSocketAddrs;
use std::sync::{Arc, Mutex};

use leyline::{
    Browser, CompressionConfig, DnsConfig, Family, RedirectAction, RedirectPolicy, Session,
    TimeoutConfig,
};
use url::Url;

use crate::config;
use crate::error::{Error, Result};

const HITS_DEFAULT: usize = 5;
const HITS_MAX: usize = 8;
const QUERY_MAX: usize = 400;

const SEARCH_URL: &str = "https://www.bing.com/search";
mod fetch;
mod results;

use fetch::{host_blocked, resolves_blocked};
use results::{Hit, flat, format_hits, parse_rss};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Format {
    Markdown,
    Text,
    Html,
}

impl Format {
    pub(crate) fn parse(value: Option<&str>) -> Result<Self> {
        match value.map(str::trim) {
            None | Some("") | Some("markdown") => Ok(Self::Markdown),
            Some("text") => Ok(Self::Text),
            Some("html") => Ok(Self::Html),
            Some(other) => Err(Error::Tool(format!(
                "web_fetch format takes markdown, text, or html, not `{other}`"
            ))),
        }
    }
}

#[derive(Clone)]
pub(crate) struct Search {
    session: Arc<Mutex<Option<Session>>>,
    count: usize,
    allowed: Vec<String>,
    excluded: Vec<String>,
    origin: Option<String>,
    logs: Arc<Mutex<Vec<tempfile::TempDir>>>,
}

impl Search {
    pub(crate) fn discover() -> Result<Self> {
        let settings = config::file()?;
        let file = &settings.search;
        let count = file.count.unwrap_or(HITS_DEFAULT).clamp(1, HITS_MAX);
        let allowed = normalize_domains(file.allowed_domains.clone().unwrap_or_default());
        let excluded = normalize_domains(file.excluded_domains.clone().unwrap_or_default());
        Ok(Self {
            session: Arc::new(Mutex::new(None)),
            count,
            allowed,
            excluded,
            origin: None,
            logs: Arc::new(Mutex::new(Vec::new())),
        })
    }

    fn http(&self) -> Result<Session> {
        let mut held = self
            .session
            .lock()
            .map_err(|error| Error::Tool(error.to_string()))?;
        if let Some(session) = held.as_ref() {
            return Ok(session.clone());
        }
        let session = client()?;
        *held = Some(session.clone());
        Ok(session)
    }

    pub(crate) async fn search(
        &self,
        query: &str,
        count: Option<usize>,
        freshness: Option<&str>,
    ) -> Result<String> {
        let query = query.trim();
        if query.is_empty() {
            return Err(Error::Tool("web_search requires query".to_string()));
        }
        if query.chars().count() > QUERY_MAX {
            return Err(Error::Tool("web_search query is too long".to_string()));
        }
        let count = count.unwrap_or(self.count).clamp(1, HITS_MAX);
        let hits = self.rss(query, freshness).await?;
        let hits = self
            .filter_hits(hits)
            .into_iter()
            .take(count)
            .collect::<Vec<_>>();
        Ok(format!("{}\nvia bing rss", format_hits(query, &hits)))
    }

    async fn rss(&self, query: &str, freshness: Option<&str>) -> Result<Vec<Hit>> {
        let endpoint = self
            .origin
            .clone()
            .unwrap_or_else(|| SEARCH_URL.to_string());
        let mut url = Url::parse(&endpoint)
            .map_err(|error| Error::Tool(format!("web_search url: {error}")))?;
        {
            let mut pairs = url.query_pairs_mut();
            pairs.append_pair("q", query);
            pairs.append_pair("format", "rss");
            if let Some(filter) = freshness_filter(freshness) {
                pairs.append_pair("filters", filter);
            }
        }
        let response = self
            .http()?
            .get(url.as_str())
            .header("accept", "application/rss+xml, application/xml")
            .send()
            .await
            .map_err(|error| Error::Tool(format!("web_search failed: {error}")))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|error| Error::Tool(format!("web_search failed: {error}")))?;
        if !status.is_success() {
            return Err(Error::Tool(format!(
                "web_search HTTP {status}: {}",
                crate::text::ellipsize(&flat(&body), 200)
            )));
        }
        if !body.contains("<rss") {
            return Err(Error::Tool(
                "web_search returned an unexpected response".to_string(),
            ));
        }
        Ok(parse_rss(&body))
    }

    fn filter_hits(&self, hits: Vec<Hit>) -> Vec<Hit> {
        let mut out = Vec::new();
        for hit in hits {
            let host = host_of(&hit.url);
            if !self.allowed.is_empty() && !self.allowed.iter().any(|d| domain_match(&host, d)) {
                continue;
            }
            if self.excluded.iter().any(|d| domain_match(&host, d)) {
                continue;
            }
            if out.iter().any(|have: &Hit| have.url == hit.url) {
                continue;
            }
            out.push(hit);
        }
        out
    }
}

fn http_builder() -> leyline::SessionBuilder {
    Session::builder()
        .browser(Browser::latest(Family::Chrome))
        .timeout(
            TimeoutConfig::new()
                .total(crate::limits::limits().web_search())
                .connect(crate::limits::limits().web_connect()),
        )
        .compression(CompressionConfig::new().max_body_size(crate::limits::limits().web_body_bytes))
        .redirect(RedirectPolicy::custom(|attempt| {
            if attempt.previous.len() >= crate::limits::limits().web_redirect_max
                || !safe_redirect(attempt.url)
            {
                RedirectAction::Stop
            } else {
                RedirectAction::Follow
            }
        }))
}

fn client() -> Result<Session> {
    crate::credentials::session(http_builder())
}

pub(super) fn fetch_targets(host: &str) -> std::result::Result<Vec<std::net::SocketAddr>, ()> {
    let addrs = match host.parse::<std::net::IpAddr>() {
        Ok(ip) => vec![std::net::SocketAddr::new(ip, 443)],
        Err(_) => match (host, 443).to_socket_addrs() {
            Ok(addrs) => addrs.collect::<Vec<_>>(),
            Err(_) => return Err(()),
        },
    };
    if addrs.is_empty() || addrs.iter().any(|addr| fetch::ip_blocked(addr.ip())) {
        return Err(());
    }
    Ok(addrs)
}

pub(super) fn pinned_session(host: &str, addrs: &[std::net::SocketAddr]) -> Result<Session> {
    crate::credentials::session(
        http_builder().dns(DnsConfig::new().resolve_host(host, addrs.iter().copied())),
    )
}

fn safe_redirect(url: &Url) -> bool {
    let host = url.host_str().unwrap_or("");
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && !host_blocked(host)
        && !resolves_blocked(host)
}

fn host_of(url: &str) -> String {
    Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_ascii_lowercase))
        .unwrap_or_default()
}

fn domain_match(host: &str, domain: &str) -> bool {
    host == domain || host.ends_with(&format!(".{domain}"))
}

fn normalize_domains(list: Vec<String>) -> Vec<String> {
    list.into_iter()
        .map(|d| d.trim().trim_start_matches('.').to_ascii_lowercase())
        .filter(|d| !d.is_empty() && !d.starts_with("http"))
        .collect()
}

fn freshness_filter(value: Option<&str>) -> Option<&'static str> {
    match value {
        Some("day") => Some("ex1:\"ez1\""),
        Some("week") => Some("ex1:\"ez2\""),
        Some("month") => Some("ex1:\"ez3\""),
        _ => None,
    }
}

#[cfg(test)]
#[path = "web_tests.rs"]
mod tests;
