use super::*;

impl Search {
    pub(crate) fn native_for_test(origin: &str) -> Result<Self> {
        Ok(Self {
            http: client()?,
            count: 5,
            allowed: Vec::new(),
            excluded: Vec::new(),
            origin: Some(origin.to_string()),
            backend: Backend::Bing,
        })
    }
}

#[test]
fn search_rss_becomes_numbered_hits() {
    let xml = r#"<?xml version="1.0"?><rss><channel>
        <item><title>Tokio &amp; Rust</title><link>https://tokio.rs/</link><description>The asynchronous runtime for Rust.</description></item>
        <item><title>Docs</title><link>https://docs.rs/tokio</link><description>API reference</description></item>
    </channel></rss>"#;
    let hits = parse_rss(xml);
    assert_eq!(hits[0].title, "Tokio & Rust");
    assert_eq!(hits[0].url, "https://tokio.rs/");
    let text = format_hits("tokio", &hits);
    assert!(text.contains("1. Tokio"));
    assert!(text.contains("https://tokio.rs"));
    assert!(text.contains("The asynchronous runtime"));
    assert!(text.contains("web_fetch"));
}

#[test]
fn duckduckgo_html_becomes_direct_numbered_hits() {
    let html = r#"<div class="result results_links">
        <a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fworld&amp;rut=x"><b>Persistent</b> worlds</a>
        <a class="result__snippet" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fworld&amp;rut=x">An <b>LLM</b> game state framework.</a>
    </div>"#;
    let hits = parse_duckduckgo(html);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].title, "Persistent worlds");
    assert_eq!(hits[0].url, "https://example.com/world");
    assert_eq!(hits[0].snippet, "An LLM game state framework.");
}

#[test]
fn empty_hits_say_zero() {
    assert_eq!(format_hits("nope", &[]), "web_search  0 hits  `nope`");
}

#[tokio::test]
#[ignore = "live network smoke check"]
async fn live_native_search_returns_links() -> Result<()> {
    let mut search = Search::native_for_test(DUCKDUCKGO_URL)?;
    search.backend = Backend::DuckDuckGo;
    let output = search
        .search(
            "LLM game master persistent world state framework",
            Some(5),
            None,
        )
        .await?;
    assert!(output.contains("web_search  5 hits"));
    assert!(output.contains("arxiv.org") || output.contains("github.com"));
    assert!(output.to_ascii_lowercase().contains("persistent"));
    Ok(())
}

#[test]
fn ssrf_blocks_private_and_localhost() {
    check_url("http://127.0.0.1/").unwrap_err();
    check_url("https://localhost/secret").unwrap_err();
    check_url("https://203.0.113.4/").unwrap_err();
    check_url("https://169.254.169.254/latest").unwrap_err();
    check_url("https://100.64.0.1/").unwrap_err();
    check_url("https://[fd12:3456:789a::1]/").unwrap_err();
    check_url("https://metadata.google.internal/").unwrap_err();
    check_url("file:///etc/passwd").unwrap_err();
    check_url("https://user:pass@example.com/").unwrap_err();
    check_url("https://docs.rs/regex").unwrap();
}

#[test]
fn html_keeps_links_and_headings() {
    let md = html_to_md(
        "<html><script>alert(1)</script><h1>Title</h1><p>Hi <a href=\"https://a.dev\">A</a></p>",
    );
    assert!(md.contains("# Title"));
    assert!(md.contains("[A](https://a.dev)"));
    assert!(!md.contains("alert"));
}

#[test]
fn unset_backend_uses_brave_when_a_key_is_set() {
    assert!(matches!(
        select_backend(None, Some("k".into())).expect("none"),
        Backend::Brave(key) if key == "k"
    ));
    assert!(matches!(
        select_backend(None, None).expect("none"),
        Backend::DuckDuckGo
    ));
    assert!(matches!(
        select_backend(Some("bing"), Some("k".into())).expect("bing"),
        Backend::Bing
    ));
    drop(select_backend(Some("brave"), None).unwrap_err());
}

#[test]
fn brave_json_becomes_hits_and_garbage_is_named() {
    let hits = parse_brave(
        r#"{"web":{"results":[{"title":"LazyLock in std::sync","url":"https://doc.rust-lang.org/std/sync/struct.LazyLock.html","description":"A value which is initialized on the first access."},{"title":"no url"}]}}"#,
    )
    .expect("parse");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].title, "LazyLock in std::sync");
    assert!(hits[0].snippet.starts_with("A value"));
    drop(parse_brave("<html>").unwrap_err());
    assert!(parse_brave("{}").expect("empty").is_empty());
}
