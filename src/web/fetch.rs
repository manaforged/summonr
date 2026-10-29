use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::sync::LazyLock;

use leyline::http::StatusCode;
use leyline::{Response, Session};
use url::Url;

use super::results::markup::{Space, attr, collapse, decode, strip, text};
use super::{Format, Search};
use summonr_core::error::{Error, Result};

pub(crate) const OUT_MAX: usize = 32 * 1024;

const PRIVATE_HOSTS: &[&str] = &[
    "localhost",
    "localhost.",
    "*.localhost",
    "*.local",
    "*.internal",
    "127.0.0.1",
    "0.0.0.0",
    "[::1]",
    "metadata.google.internal",
];

pub(crate) static PRIVATE_URL_PATTERNS: LazyLock<Vec<String>> = LazyLock::new(|| {
    PRIVATE_HOSTS
        .iter()
        .flat_map(|host| [format!("*://{host}/*"), format!("*://{host}:*/*")])
        .collect()
});

fn header(url: &Url, body: &str) -> String {
    format!("web_fetch {url}\n{body}")
}

impl Search {
    pub(crate) async fn fetch(&self, raw: &str, format: Format) -> Result<String> {
        let url = check_url(raw)?;
        let host = url.host_str().unwrap_or("").to_string();
        let addrs = public_targets(&host)
            .await
            .ok_or_else(|| Error::Tool("web_fetch: that host is not allowed".to_string()))?;
        let session = super::pinned_session(&host, &addrs)?;
        let response = Self::request(&session, &url).await?;
        if let Some(browser) = self
            .browser()
            .filter(|_| super::wall().status(response.status().as_u16()))
        {
            return Ok(header(&url, &browser.render(&url, format).await?));
        }
        let ctype = response
            .header("content-type")
            .unwrap_or("")
            .to_ascii_lowercase();
        validate(&response, &ctype)?;
        let text = response
            .text()
            .await
            .map_err(|error| Error::Tool(format!("web_fetch failed: {error}")))?;
        if text.contains('\0') {
            return Err(Error::Tool("web_fetch: binary body".to_string()));
        }
        let body = page_body(&text, &ctype, format);
        if let Some(browser) = self.browser().filter(|_| super::wall().body(&body, &text)) {
            return Ok(header(&url, &browser.render(&url, format).await?));
        }
        Ok(header(&url, &body))
    }

    async fn request(session: &Session, url: &Url) -> Result<Response> {
        session
            .get(url.as_str())
            .timeout(crate::limits::limits().web_fetch())
            .header(
                "accept",
                "text/markdown, text/plain, text/html;q=0.9, */*;q=0.1",
            )
            .send()
            .await
            .map_err(|error| Error::Tool(format!("web_fetch failed: {error}")))
    }
}

fn validate(response: &Response, ctype: &str) -> Result<()> {
    let status = response.status();
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        return Err(Error::Tool(format!(
            "web_fetch {status}: private or authenticated pages are out of scope"
        )));
    }
    if !status.is_success() {
        return Err(Error::Tool(format!("web_fetch HTTP {status}")));
    }
    if ctype.starts_with("image/") || ctype.starts_with("video/") || ctype.starts_with("audio/") {
        return Err(Error::Tool(
            "web_fetch: that URL is not a text page".to_string(),
        ));
    }
    Ok(())
}

pub(super) fn page_body(text: &str, ctype: &str, format: Format) -> String {
    if !(ctype.contains("html") || looks_like_html(text)) {
        return text.trim().to_string();
    }
    match format {
        Format::Markdown => html_to_md(text),
        Format::Text => html_to_text(text),
        Format::Html => text.trim().to_string(),
    }
}

pub(crate) fn check_url(raw: &str) -> Result<Url> {
    let trimmed = raw.trim();
    let candidate = if let Some(rest) = trimmed.strip_prefix("http://") {
        format!("https://{rest}")
    } else {
        trimmed.to_string()
    };
    let url = Url::parse(&candidate)
        .map_err(|error| Error::Tool(format!("web_fetch: URL is invalid: {error}")))?;
    if url.scheme() != "https" {
        return Err(Error::Tool("web_fetch: only https URLs".to_string()));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(Error::Tool(
            "web_fetch: URL must not include credentials".to_string(),
        ));
    }
    let host = url.host_str().unwrap_or("");
    if host_blocked(host) {
        return Err(Error::Tool(
            "web_fetch: that host is not allowed".to_string(),
        ));
    }
    Ok(url)
}

pub(super) fn host_blocked(host: &str) -> bool {
    let host = host
        .trim_matches(|c| c == '[' || c == ']')
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if host.is_empty() {
        return true;
    }
    // complexity: bounded by the fixed PRIVATE_HOSTS list
    let named = PRIVATE_HOSTS.iter().any(|entry| {
        let entry = entry
            .trim_matches(|c| c == '[' || c == ']')
            .trim_end_matches('.');
        match entry.strip_prefix('*') {
            Some(suffix) => host.ends_with(suffix),
            None => host == entry,
        }
    });
    named || host.parse::<IpAddr>().is_ok_and(ip_blocked)
}

pub(super) fn ip_blocked(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let oct = v4.octets();
            oct[0] == 0
                || v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || (oct[0] == 100 && oct[1] & 0xc0 == 64)
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_unique_local()
                || (v6.segments()[0] & 0xffc0) == 0xfe80
                || v6
                    .to_ipv4_mapped()
                    .is_some_and(|v4| ip_blocked(IpAddr::V4(v4)))
        }
    }
}

pub(super) fn fetch_targets(host: &str) -> Option<Vec<SocketAddr>> {
    let addrs: Vec<SocketAddr> = match host.parse::<IpAddr>() {
        Ok(ip) => vec![SocketAddr::new(ip, 443)],
        Err(_) => (host, 443).to_socket_addrs().ok()?.collect(),
    };
    (!addrs.is_empty() && !addrs.iter().any(|addr| ip_blocked(addr.ip()))).then_some(addrs)
}

pub(super) async fn public_targets(host: &str) -> Option<Vec<SocketAddr>> {
    let host = host.to_string();
    tokio::task::spawn_blocking(move || fetch_targets(&host))
        .await
        .ok()
        .flatten()
}

fn looks_like_html(text: &str) -> bool {
    let head: String = text
        .chars()
        .take(256)
        .collect::<String>()
        .to_ascii_lowercase();
    head.contains("<html") || head.contains("<!doctype html") || head.contains("<head")
}

pub(super) fn html_to_md(html: &str) -> String {
    let mut without = strip_blocks(html, "script");
    without = strip_blocks(&without, "style");
    without = strip_blocks(&without, "noscript");
    let mut out = String::with_capacity(without.len());
    let bytes = without.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<'
            && let Some((tag, href, end)) = parse_tag(&without, i)
        {
            {
                match tag.as_str() {
                    "br" | "p" | "div" | "tr" | "li" | "/p" | "/div" | "/tr" | "/h1" | "/h2"
                    | "/h3" | "/li" => out.push('\n'),
                    "h1" => out.push_str("\n# "),
                    "h2" => out.push_str("\n## "),
                    "h3" => out.push_str("\n### "),
                    "a" => {
                        if let Some(href) = href {
                            out.push('[');
                            i = end;
                            let (text, close) = take_until_close(&without, i, "a");
                            out.push_str(&text);
                            out.push_str("](");
                            out.push_str(href);
                            out.push(')');
                            i = close;
                            continue;
                        }
                    }
                    _ => {}
                }
                i = end;
                continue;
            }
        }
        let rest = &without[i..];
        let ch = rest.chars().next().unwrap_or('?');
        out.push(ch);
        i += ch.len_utf8();
    }
    decode(&collapse(&out, Space::Lines))
}

pub(super) fn html_to_text(html: &str) -> String {
    let without = strip_blocks(
        &strip_blocks(&strip_blocks(html, "script"), "style"),
        "noscript",
    );
    decode(&collapse(&strip(&without), Space::Lines))
}

fn strip_blocks(html: &str, tag: &str) -> String {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let lower = html.to_ascii_lowercase();
    let mut out = String::new();
    let mut from = 0;
    while let Some(rel) = lower[from..].find(&open) {
        let start = from + rel;
        out.push_str(&html[from..start]);
        let after = start + open.len();
        if let Some(end_rel) = lower[after..].find(&close) {
            from = after + end_rel + close.len();
        } else {
            from = html.len();
        }
    }
    out.push_str(&html[from..]);
    out
}

fn parse_tag(html: &str, at: usize) -> Option<(String, Option<&str>, usize)> {
    let bytes = html.as_bytes();
    let close = bytes[at + 1..].iter().position(|&b| b == b'>')? + at + 1;
    let inner = html[at + 1..close].trim();
    let name = inner
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_end_matches('/')
        .to_ascii_lowercase();
    let href = if name == "a" {
        attr(inner, "href")
    } else {
        None
    };
    Some((name, href, close + 1))
}

fn take_until_close(html: &str, from: usize, tag: &str) -> (String, usize) {
    let close = format!("</{tag}>");
    let lower = html[from..].to_ascii_lowercase();
    if let Some(at) = lower.find(&close) {
        (text(&html[from..from + at]), from + at + close.len())
    } else {
        (text(&html[from..]), html.len())
    }
}
