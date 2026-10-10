use url::Url;

use super::safe_redirect;

fn follows(url: &str) -> bool {
    safe_redirect(&Url::parse(url).unwrap())
}

#[test]
fn a_redirect_stops_at_credentials_plain_http_and_blocked_hosts() {
    assert!(follows("https://203.0.113.7/next"));
    assert!(!follows("https://user@203.0.113.7/next"));
    assert!(!follows("https://:secret@203.0.113.7/next"));
    assert!(!follows("http://203.0.113.7/next"));
    assert!(!follows("https://trusted.example@127.0.0.1/next"));
    assert!(!follows("https://127.0.0.1/next"));
}

#[test]
fn a_fetch_rejects_an_address_in_the_unspecified_block() {
    assert!(super::fetch::host_blocked("0.1.2.3"));
    assert!(!super::fetch::host_blocked("203.0.113.7"));
}

const DDG: &str = r#"<div class="result results_links results_links_deep web-result "><div class="links_main links_deep result__body"><h2 class="result__title"><a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fdocs.rs%2Ftokio%2Flatest%2Ftokio%2Fmacro.select.html&amp;rut=9d03">select in <b>tokio</b> - Rust &amp; Docs.rs</a></h2><a class="result__snippet" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fdocs.rs%2F">Waits on <b>multiple</b> concurrent branches.</a></div></div>
<div class="result results_links results_links_deep web-result "><div class="links_main links_deep result__body"><h2 class="result__title"><a rel="nofollow" class="result__a" href="https://duckduckgo.com/y.js?ad_domain=example.com">Ad</a></h2></div></div>"#;

const BRAVE: &str = r#"<div class="snippet svelte-jmfu5f" data-pos="0" data-type="web" data-keynav="true"><div class="result-content svelte-1rq4ngz"><a href="https://stackoverflow.com/questions/72263644" target="_self" class="svelte-14r20fy l1"><div class="site-name-content svelte-on1hvy"><div class="desktop-small-semibold">Stack Overflow</div></div> <div class="title search-snippet-title line-clamp-1 svelte-14r20fy" title="Tokio select">rust - Tokio select macro</div></a> <div class="generic-snippet svelte-1cwdgg3"><div class="content svelte-1cwdgg3">Each <strong>branch</strong> &lt;runs&gt; concurrently.</div></div></div></div>
<div class="snippet svelte-jmfu5f" data-pos="1" data-type="web" data-keynav="true"><a href="https://search.brave.com/goggles" class="svelte-14r20fy l1"><div class="title search-snippet-title">Goggles</div></a></div>"#;

fn engine(name: &str) -> &'static super::results::Engine {
    super::results::engines()
        .unwrap()
        .iter()
        .find(|engine| engine.name == name)
        .unwrap()
}

#[test]
fn engines_are_tried_duckduckgo_then_brave() {
    let names = super::results::engines()
        .unwrap()
        .iter()
        .map(|engine| engine.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, ["duckduckgo", "brave"]);
}

#[test]
fn a_duckduckgo_page_yields_the_unwrapped_url_title_and_snippet() {
    let hits = super::results::parse_page(DDG, engine("duckduckgo"));
    assert_eq!(hits.len(), 1);
    assert_eq!(
        hits[0].url,
        "https://docs.rs/tokio/latest/tokio/macro.select.html"
    );
    assert_eq!(hits[0].title, "select in tokio - Rust & Docs.rs");
    assert_eq!(hits[0].snippet, "Waits on multiple concurrent branches.");
}

#[test]
fn a_brave_page_yields_results_and_skips_its_own_links() {
    let hits = super::results::parse_page(BRAVE, engine("brave"));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].url, "https://stackoverflow.com/questions/72263644");
    assert_eq!(hits[0].title, "rust - Tokio select macro");
    assert_eq!(hits[0].snippet, "Each branch <runs> concurrently.");
}

#[test]
fn freshness_maps_to_the_engine_parameter() {
    let url = engine("brave")
        .request("tokio select", Some(super::results::Window::Week))
        .unwrap();
    assert_eq!(
        url.as_str(),
        "https://search.brave.com/search?q=tokio+select&tf=pw"
    );
    let url = engine("duckduckgo")
        .request("tokio", Some(super::results::Window::Day))
        .unwrap();
    assert_eq!(
        url.as_str(),
        "https://html.duckduckgo.com/html/?q=tokio&df=d"
    );
    let url = engine("duckduckgo").request("tokio", None).unwrap();
    assert_eq!(url.as_str(), "https://html.duckduckgo.com/html/?q=tokio");
}

#[test]
fn freshness_rejects_an_unknown_window() {
    let error = super::results::Window::parse(Some("year")).unwrap_err();
    assert!(error.to_string().contains("not `year`"), "{error}");
    assert_eq!(
        super::results::Window::parse(Some(" month ")).unwrap(),
        Some(super::results::Window::Month)
    );
}

#[test]
fn the_browser_reaches_only_https_and_inert_pages() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        assert!(!super::reachable("http://example.com/").await);
        assert!(!super::reachable("http://127.0.0.1:8080/").await);
        assert!(!super::reachable("https://127.0.0.1/").await);
        assert!(!super::reachable("file:///etc/passwd").await);
        assert!(super::reachable("about:blank").await);
    });
}
