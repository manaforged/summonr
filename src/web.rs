use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use leyline::{
    Browser, CompressionConfig, DnsConfig, Family, RedirectAction, RedirectPolicy, Session,
};
use url::Url;

use crate::config;
use summonr_core::error::{Error, Result};

const HITS_DEFAULT: usize = 5;
const HITS_MAX: usize = 8;
const QUERY_MAX: usize = 400;
mod fetch;
mod results;

pub(crate) use fetch::OUT_MAX;
pub(crate) use fetch::PRIVATE_URL_PATTERNS;
use fetch::{check_url, fetch_targets, host_blocked, public_targets};
const INERT_SCHEMES: [&str; 3] = ["about", "data", "chrome-error"];
use results::markup::{Space, collapse};
pub(crate) use results::wall;
use results::{Engine, Hit, Window, engines, format_hits, parse_page};

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
    browser: Arc<Mutex<Option<crate::browser::Browser>>>,
    cooldowns: Arc<Cooldowns>,
}

#[derive(Default)]
pub(crate) struct Cooldowns(Mutex<HashMap<String, Instant>>);

impl Cooldowns {
    pub(crate) fn block(&self, name: &str, until: Instant) {
        if let Ok(mut held) = self.0.lock() {
            held.insert(name.to_string(), until);
        }
    }

    pub(crate) fn blocked(&self, name: &str, now: Instant) -> bool {
        self.0
            .lock()
            .ok()
            .and_then(|held| held.get(name).copied())
            .is_some_and(|until| until > now)
    }
}

enum Page {
    Hits(Vec<results::Hit>),
    Blocked(Duration),
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
            browser: Arc::new(Mutex::new(None)),
            cooldowns: Arc::default(),
        })
    }

    pub(crate) fn set_browser(&self, browser: Option<crate::browser::Browser>) {
        if let Ok(mut held) = self.browser.lock() {
            *held = browser;
        }
    }

    fn browser(&self) -> Option<crate::browser::Browser> {
        self.browser.lock().ok().and_then(|held| held.clone())
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
        let freshness = Window::parse(freshness)?;
        let mut failure = None;
        let mut found = None;
        let mut tried = 0;
        for engine in engines() {
            if self.cooldowns.blocked(&engine.name, Instant::now()) {
                continue;
            }
            tried += 1;
            match self.page(engine, query, freshness).await {
                Ok(Page::Hits(hits)) if !hits.is_empty() => {
                    found = Some((engine, hits));
                    break;
                }
                Ok(Page::Hits(_)) => {}
                Ok(Page::Blocked(wait)) => {
                    self.cooldowns.block(&engine.name, Instant::now() + wait);
                }
                Err(error) => {
                    failure.get_or_insert(error);
                }
            }
        }
        if found.is_none() && failure.is_none() && tried < engines().len() {
            return Err(Error::Tool(
                "web_search engines are blocked after rate limits; try again in a minute"
                    .to_string(),
            ));
        }
        let Some((engine, hits)) = found else {
            return match failure {
                Some(error) => Err(error),
                None => Ok(format_hits(query, &[])),
            };
        };
        let hits = self
            .filter_hits(hits)
            .into_iter()
            .take(count)
            .collect::<Vec<_>>();
        Ok(format!(
            "{}\nvia {}",
            format_hits(query, &hits),
            engine.name
        ))
    }

    async fn page(&self, engine: &Engine, query: &str, freshness: Option<Window>) -> Result<Page> {
        let url = engine.request(query, freshness)?;
        let response = self
            .http()?
            .get(url.as_str())
            .header("accept", "text/html,application/xhtml+xml")
            .send()
            .await
            .map_err(|error| Error::Tool(format!("web_search {} failed: {error}", engine.name)))?;
        let status = response.status();
        let wait = response
            .header("retry-after")
            .and_then(|value| value.trim().parse::<u64>().ok())
            .map_or_else(|| engine.cooldown(), Duration::from_secs);
        let body = response
            .text()
            .await
            .map_err(|error| Error::Tool(format!("web_search {} failed: {error}", engine.name)))?;
        if engine.blocked(status.as_u16(), &body) {
            return Ok(Page::Blocked(wait));
        }
        if !status.is_success() {
            return Err(Error::Tool(format!(
                "web_search {} HTTP {status}: {}",
                engine.name,
                crate::display::ellipsize(&collapse(&body, Space::Flat), 200)
            )));
        }
        Ok(Page::Hits(parse_page(&body, engine)))
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
    summonr_auth::builder(
        crate::limits::limits().web_search(),
        crate::limits::limits().web_connect(),
    )
    .browser(Browser::latest(Family::Chrome))
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
    crate::credentials::session(
        http_builder(),
        crate::limits::limits()
            .retry_policy(crate::limits::Reach::Remote)
            .retry_on([leyline::RetryTrigger::ConnectionError]),
    )
}

pub(super) fn pinned_session(host: &str, addrs: &[std::net::SocketAddr]) -> Result<Session> {
    crate::credentials::session(
        http_builder().dns(DnsConfig::new().resolve_host(host, addrs.iter().copied())),
        crate::limits::limits().retry_policy(crate::limits::Reach::Remote),
    )
}

pub(crate) async fn allowed_url(raw: &str) -> Result<String> {
    let url = check_url(raw)?;
    public_targets(url.host_str().unwrap_or(""))
        .await
        .ok_or_else(|| Error::Tool("browser: that host is not allowed".to_string()))?;
    Ok(url.to_string())
}

pub(crate) async fn reachable(raw: &str) -> bool {
    let Ok(url) = Url::parse(raw) else {
        return false;
    };
    if INERT_SCHEMES.contains(&url.scheme()) {
        return true;
    }
    let host = url.host_str().unwrap_or("");
    url.scheme() == "https" && !host_blocked(host) && public_targets(host).await.is_some()
}

fn safe_redirect(url: &Url) -> bool {
    let host = url.host_str().unwrap_or("");
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && !host_blocked(host)
        && fetch_targets(host).is_some()
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

#[cfg(test)]
#[path = "web_tests.rs"]
mod tests;
