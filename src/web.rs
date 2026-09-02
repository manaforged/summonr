use std::time::Duration;

use crate::config;
use crate::error::{Error, Result};

const SEARCH_TIMEOUT: Duration = Duration::from_secs(15);
const HITS_DEFAULT: usize = 5;
const HITS_MAX: usize = 8;
const QUERY_MAX: usize = 400;

const SEARCH_URL: &str = "https://www.bing.com/search";
const BRAVE_URL: &str = "https://api.search.brave.com/res/v1/web/search";
const DUCKDUCKGO_URL: &str = "https://html.duckduckgo.com/html/";

#[derive(Clone, Debug)]
enum Backend {
    Brave(String),
    DuckDuckGo,
    Bing,
}

fn select_backend(backend: Option<&str>, key: Option<String>) -> Result<Backend> {
    match backend.map(str::trim) {
        Some("brave") => key
            .ok_or_else(|| {
                Error::Config(
                    "[search] backend = \"brave\" needs api_key or BRAVE_API_KEY".to_string(),
                )
            })
            .map(Backend::Brave),
        None => Ok(key.map_or(Backend::DuckDuckGo, Backend::Brave)),
        Some("duckduckgo") | Some("ddg") => Ok(Backend::DuckDuckGo),
        Some("bing") | Some("bing-rss") => Ok(Backend::Bing),
        Some(other) => Err(Error::Config(format!(
            "[search] backend takes brave, duckduckgo, or bing, not `{other}`"
        ))),
    }
}

mod fetch;
mod results;

#[cfg(test)]
use fetch::{check_url, html_to_md};
use fetch::{host_blocked, resolves_blocked};
use results::{Hit, clip, format_hits, parse_brave, parse_duckduckgo, parse_rss};

#[derive(Clone)]
pub(crate) struct Search {
    http: reqwest::Client,
    count: usize,
    allowed: Vec<String>,
    excluded: Vec<String>,
    origin: Option<String>,
    backend: Backend,
}

impl Search {
    pub(crate) fn discover() -> Result<Self> {
        let file = config::load()?.search;
        let count = file.count.unwrap_or(HITS_DEFAULT).clamp(1, HITS_MAX);
        let allowed = normalize_domains(file.allowed_domains.unwrap_or_default());
        let excluded = normalize_domains(file.excluded_domains.unwrap_or_default());
        let key = file
            .api_key
            .filter(|key| !key.trim().is_empty())
            .or_else(|| std::env::var("BRAVE_API_KEY").ok())
            .filter(|key| !key.trim().is_empty());
        let backend = select_backend(file.backend.as_deref(), key)?;
        Ok(Self {
            http: client()?,
            count,
            allowed,
            excluded,
            origin: None,
            backend,
        })
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
        let (hits, via) = match &self.backend {
            Backend::Brave(key) => (self.brave(query, count, freshness, key).await?, "brave"),
            Backend::DuckDuckGo => (
                self.duckduckgo(query, freshness).await?,
                "duckduckgo html — set [search] backend = \"brave\" with api_key for an API-backed search",
            ),
            Backend::Bing => (self.rss(query, freshness).await?, "bing rss"),
        };
        let hits = self
            .filter_hits(hits)
            .into_iter()
            .take(count)
            .collect::<Vec<_>>();
        Ok(format!("{}\nvia {via}", format_hits(query, &hits)))
    }

    async fn brave(
        &self,
        query: &str,
        count: usize,
        freshness: Option<&str>,
        key: &str,
    ) -> Result<Vec<Hit>> {
        let endpoint = self.origin.clone().unwrap_or_else(|| BRAVE_URL.to_string());
        let mut url = reqwest::Url::parse(&endpoint)
            .map_err(|error| Error::Tool(format!("web_search url: {error}")))?;
        {
            let mut pairs = url.query_pairs_mut();
            pairs.append_pair("q", query);
            pairs.append_pair("count", &count.to_string());
            if let Some(filter) = match freshness {
                Some("day") => Some("pd"),
                Some("week") => Some("pw"),
                Some("month") => Some("pm"),
                _ => None,
            } {
                pairs.append_pair("freshness", filter);
            }
        }
        let response = self
            .http
            .get(url)
            .header(reqwest::header::ACCEPT, "application/json")
            .header("X-Subscription-Token", key)
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
                clip(&body, 200)
            )));
        }
        parse_brave(&body)
    }

    async fn duckduckgo(&self, query: &str, freshness: Option<&str>) -> Result<Vec<Hit>> {
        let endpoint = self
            .origin
            .clone()
            .unwrap_or_else(|| DUCKDUCKGO_URL.to_string());
        let mut url = reqwest::Url::parse(&endpoint)
            .map_err(|error| Error::Tool(format!("web_search url: {error}")))?;
        {
            let mut pairs = url.query_pairs_mut();
            pairs.append_pair("q", query);
            if let Some(filter) = duckduckgo_freshness(freshness) {
                pairs.append_pair("df", filter);
            }
        }
        let response = self
            .http
            .get(url)
            .header(reqwest::header::ACCEPT, "text/html")
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
                clip(&body, 200)
            )));
        }
        let hits = parse_duckduckgo(&body);
        if hits.is_empty() && !body.contains("No results.") {
            return Err(Error::Tool(
                "web_search returned an unexpected response".to_string(),
            ));
        }
        Ok(hits)
    }

    async fn rss(&self, query: &str, freshness: Option<&str>) -> Result<Vec<Hit>> {
        let endpoint = self
            .origin
            .clone()
            .unwrap_or_else(|| SEARCH_URL.to_string());
        let mut url = reqwest::Url::parse(&endpoint)
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
            .http
            .get(url)
            .header(
                reqwest::header::ACCEPT,
                "application/rss+xml, application/xml",
            )
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
                clip(&body, 200)
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

fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(SEARCH_TIMEOUT)
        .connect_timeout(Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 {
                return attempt.stop();
            }
            let url = attempt.url();
            let host = url.host_str().unwrap_or("");
            if url.scheme() != "https"
                || !url.username().is_empty()
                || url.password().is_some()
                || host_blocked(host)
                || resolves_blocked(host)
            {
                attempt.stop()
            } else {
                attempt.follow()
            }
        }))
        .user_agent(format!("summon/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| Error::Tool(format!("http client: {error}")))
}

fn host_of(url: &str) -> String {
    reqwest::Url::parse(url)
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

fn duckduckgo_freshness(value: Option<&str>) -> Option<&'static str> {
    match value {
        Some("day") => Some("d"),
        Some("week") => Some("w"),
        Some("month") => Some("m"),
        _ => None,
    }
}

#[cfg(test)]
#[path = "web_tests.rs"]
mod tests;
