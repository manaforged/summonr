use std::net::{IpAddr, ToSocketAddrs};
use std::path::PathBuf;

use leyline::http::StatusCode;
use leyline::{Response, Session};
use url::Url;

use super::{Format, Search};
use crate::browser::wall::WALL;
use crate::error::{Error, Result};

pub(crate) const OUT_MAX: usize = 32 * 1024;
const PAGE_PREFIX: &str = "page-";
const PAGE_SUFFIX: &str = ".md";

impl Search {
    pub(crate) async fn fetch(&self, raw: &str, format: Format) -> Result<String> {
        let url = check_url(raw)?;
        let host = url.host_str().unwrap_or("").to_string();
        let lookup = {
            let host = host.clone();
            tokio::task::spawn_blocking(move || super::fetch_targets(&host))
                .await
                .unwrap_or(Err(()))
        };
        let addrs =
            lookup.map_err(|()| Error::Tool("web_fetch: that host is not allowed".to_string()))?;
        let session = super::pinned_session(&host, &addrs)?;
        let response = Self::request(&session, &url).await?;
        if let Some(browser) = self
            .browser()
            .filter(|_| WALL.status(response.status().as_u16()))
        {
            return Ok(self.output(&url, &browser.render(&url).await?));
        }
        let ctype = response
            .header("content-type")
            .unwrap_or("")
            .to_ascii_lowercase();
        validate(&response, &ctype)?;
        let bytes = response
            .bytes()
            .await
            .map_err(|error| Error::Tool(format!("web_fetch failed: {error}")))?;
        if bytes.contains(&0) {
            return Err(Error::Tool("web_fetch: binary body".to_string()));
        }
        let text = std::str::from_utf8(&bytes)
            .map_err(|error| Error::Tool(format!("web_fetch: binary body: {error}")))?;
        let body = page_body(text, &ctype, format);
        if let Some(browser) = self.browser().filter(|_| WALL.body(&body, text)) {
            return Ok(self.output(&url, &browser.render(&url).await?));
        }
        Ok(self.output(&url, &body))
    }

    pub(super) fn output(&self, url: &Url, body: &str) -> String {
        let mut out = format!("web_fetch {url}\n");
        if body.chars().count() <= OUT_MAX {
            out.push_str(body);
            return out;
        }
        out.extend(body.chars().take(OUT_MAX));
        out.push_str("\n… truncated");
        out.push_str(&match self.keep(body) {
            Ok(path) => format!("\n[full text: {} ({} bytes)]", path.display(), body.len()),
            Err(error) => format!("\n[full text not kept: {error}]"),
        });
        out
    }

    fn keep(&self, body: &str) -> Result<PathBuf> {
        let mut dirs = self.logs.lock().map_err(|e| Error::Tool(e.to_string()))?;
        if dirs.is_empty() {
            dirs.push(tempfile::tempdir()?);
        }
        let dir = dirs
            .first()
            .map(tempfile::TempDir::path)
            .ok_or_else(|| Error::Tool("web_fetch: page directory is unavailable".to_string()))?;
        Ok(crate::durable::store::keep(
            dir,
            PAGE_PREFIX,
            PAGE_SUFFIX,
            body.as_bytes(),
        )?)
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
        .to_ascii_lowercase();
    if host.is_empty()
        || host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || host.ends_with(".internal")
        || host == "metadata.google.internal"
    {
        return true;
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        return ip_blocked(ip);
    }
    false
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

pub(super) fn resolves_blocked(host: &str) -> bool {
    if host.parse::<IpAddr>().is_ok() {
        return false;
    }
    (host, 443).to_socket_addrs().map_or(true, |addrs| {
        let addrs: Vec<_> = addrs.collect();
        addrs.is_empty() || addrs.iter().any(|addr| ip_blocked(addr.ip()))
    })
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
                            out.push_str(&href);
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
    decode_entities(&collapse_space(&out))
}

pub(super) fn html_to_text(html: &str) -> String {
    let without = strip_blocks(
        &strip_blocks(&strip_blocks(html, "script"), "style"),
        "noscript",
    );
    let mut out = String::with_capacity(without.len());
    let mut in_tag = false;
    for ch in without.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    decode_entities(&collapse_space(&out))
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

fn parse_tag(html: &str, at: usize) -> Option<(String, Option<String>, usize)> {
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

fn attr(tag: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=");
    let lower = tag.to_ascii_lowercase();
    let at = lower.find(&needle)?;
    let rest = tag[at + needle.len()..].trim_start();
    let quote = rest.chars().next()?;
    if quote == '"' || quote == '\'' {
        let end = rest[1..].find(quote)?;
        Some(rest[1..1 + end].to_string())
    } else {
        Some(
            rest.split_whitespace()
                .next()
                .unwrap_or("")
                .trim_end_matches('/')
                .to_string(),
        )
    }
}

fn take_until_close(html: &str, from: usize, tag: &str) -> (String, usize) {
    let close = format!("</{tag}>");
    let lower = html[from..].to_ascii_lowercase();
    if let Some(at) = lower.find(&close) {
        let text = strip_tags(&html[from..from + at]);
        (text, from + at + close.len())
    } else {
        (strip_tags(&html[from..]), html.len())
    }
}

fn strip_tags(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    decode_entities(&out)
}

fn decode_entities(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

fn collapse_space(text: &str) -> String {
    let mut out = String::new();
    let mut blank = 0;
    for line in text.lines() {
        let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
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
