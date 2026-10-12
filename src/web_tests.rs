use url::Url;

use super::fetch::host_blocked;
use super::results::{Window, engines, parse_page};
use super::{PRIVATE_URL_PATTERNS, safe_redirect};

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
fn a_fetch_blocks_the_unspecified_block_and_trailing_dot_hosts() {
    assert!(host_blocked("0.1.2.3"));
    assert!(!host_blocked("203.0.113.7"));
    assert!(host_blocked("localhost."));
    assert!(host_blocked("127.0.0.1."));
    assert!(!host_blocked("example.com."));
}

fn glob(pattern: &str, text: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == text,
        Some((head, tail)) => {
            let Some(rest) = text.strip_prefix(head) else {
                return false;
            };
            (0..=rest.len()).any(|at| glob(tail, &rest[at..]))
        }
    }
}

fn private(url: &str) -> bool {
    PRIVATE_URL_PATTERNS
        .iter()
        .any(|pattern| glob(pattern, url))
}

#[test]
fn the_private_url_patterns_reach_the_denied_host_literals() {
    for url in [
        "http://localhost/a",
        "https://sub.localhost/a",
        "https://127.0.0.1:9/a",
        "http://localhost.:8080/a",
        "http://localhost./a",
        "http://[::1]:80/a",
        "http://0.0.0.0/a",
        "https://metadata.google.internal/a",
    ] {
        assert!(private(url), "{url}");
    }
    for url in [
        "https://10.0.0.5/a",
        "http://192.168.1.10/a",
        "https://169.254.3.4/a",
        "http://[fd00::1]/a",
        "https://example.com/a",
        "https://8.8.8.8/a",
        "https://172.32.0.1/a",
        "https://example.com/?next=http%3A%2F%2F127.0.0.1%2F",
    ] {
        assert!(!private(url), "{url}");
    }
}

const DDG: &str = r#"<div class="result results_links results_links_deep web-result "><div class="links_main links_deep result__body"><h2 class="result__title"><a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fdocs.rs%2Ftokio%2Flatest%2Ftokio%2Fmacro.select.html&amp;rut=9d03">select in <b>tokio</b> - Rust &amp; Docs.rs</a></h2><a class="result__snippet" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fdocs.rs%2F">Waits on <b>multiple</b> concurrent branches.</a></div></div>
<div class="result results_links results_links_deep web-result "><div class="links_main links_deep result__body"><h2 class="result__title"><a rel="nofollow" class="result__a" href="https://duckduckgo.com/y.js?ad_domain=example.com">Ad</a></h2></div></div>"#;

const BRAVE: &str = r#"<div class="snippet svelte-jmfu5f" data-pos="0" data-type="web" data-keynav="true"><div class="result-content svelte-1rq4ngz"><a href="https://stackoverflow.com/questions/72263644" target="_self" class="svelte-14r20fy l1"><div class="site-name-content svelte-on1hvy"><div class="desktop-small-semibold">Stack Overflow</div></div> <div class="title search-snippet-title line-clamp-1 svelte-14r20fy" title="Tokio select">rust - Tokio select macro</div></a> <div class="generic-snippet svelte-1cwdgg3"><div class="content svelte-1cwdgg3">Each <strong>branch</strong> &lt;runs&gt; concurrently.</div></div></div></div>
<div class="snippet svelte-jmfu5f" data-pos="1" data-type="web" data-keynav="true"><a href="https://search.brave.com/goggles" class="svelte-14r20fy l1"><div class="title search-snippet-title">Goggles</div></a></div>"#;

const BING: &str = r#"<ol id="b_results"><li class="b_algo" data-id iid=SERP.5344><link rel="stylesheet" href="https://r.bing.com/rp/a.css" type="text/css"/><div class="b_tpcn"><a class="tilk" href="https://serde.rs/"><div class="tptxt">serde.rs</div></a></div><h2><a target="_blank" href="https://www.bing.com/ck/a?!&amp;&amp;p=1ecb&amp;ptn=3&amp;u=a1aHR0cHM6Ly9zZXJkZS5ycy9maWVsZC1hdHRycy5odG1s&amp;ntb=1" h="ID=SERP,5128.2">Field attributes &#183; <strong>Serde</strong></a></h2><div class="b_caption"><p class="b_lineclamp2">Serialize and deserialize this field with the given name.</p></div></li><li class="b_algo"><h2><a href="https://www.bing.com/ck/a?!&amp;&amp;u=a1aHR0cHM6Ly93d3cuYmluZy5jb20vbmV3cw&amp;ntb=1">Bing News</a></h2></li></ol>"#;

const YAHOO: &str = r#"<li class="first"><div class="dd fst algo algo-sr relsrch richAlgo"><div class="compTitle options-toggle"><a class="d-ib va-top" target="_blank" href="https://r.search.yahoo.com/_ylt=A2RR;_ylu=Y29s/RV=2/RE=1792959646/RO=10/RU=https%3a%2f%2fserde.rs%2ffield-attrs.html/RK=2/RS=Ckx3-"><span class="d-ib va-mid"><span class="fc-141414 d-b">Serde</span>https://serde.rs › field-attrs</span><h3 class="title fc-2015C2-imp"><span class="d-b fz-20">Field attributes &middot; Serde</span></h3></a></div><div class="compText aAbs"><p class="fc-dustygray"> Learn how to use<b> #[serde(rename = &quot;name&quot;)]</b> attributes. </p></div></div></li>
<li><div class="dd algo algo-sr"><div class="compTitle"><a href="https://r.search.yahoo.com/_ylt=x/RV=2/RU=https%3a%2f%2fsearch.yahoo.com%2fpreferences/RK=2/RS=y-"><h3 class="title">Preferences</h3></a></div></div></li>"#;

fn engine(name: &str) -> &'static super::results::Engine {
    engines()
        .unwrap()
        .iter()
        .find(|engine| engine.name == name)
        .unwrap()
}

#[test]
fn engines_are_tried_bing_duckduckgo_yahoo_then_brave() {
    let names = engines()
        .unwrap()
        .iter()
        .map(|engine| engine.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, ["bing", "duckduckgo", "yahoo", "brave"]);
}

#[test]
fn a_bing_page_yields_the_decoded_url_title_and_snippet_and_skips_its_own_links() {
    let hits = parse_page(BING, engine("bing"));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].url, "https://serde.rs/field-attrs.html");
    assert_eq!(hits[0].title, "Field attributes · Serde");
    assert_eq!(
        hits[0].snippet,
        "Serialize and deserialize this field with the given name."
    );
    let url = engine("bing").request("serde", Some(Window::Week)).unwrap();
    assert_eq!(
        url.as_str(),
        "https://www.bing.com/search?q=serde&filters=ex1%3A%22ez2%22"
    );
}

#[test]
fn a_yahoo_page_yields_the_url_from_its_redirect_path_and_skips_its_own_links() {
    let hits = parse_page(YAHOO, engine("yahoo"));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].url, "https://serde.rs/field-attrs.html");
    assert_eq!(hits[0].title, "Field attributes · Serde");
    assert_eq!(
        hits[0].snippet,
        "Learn how to use #[serde(rename = \"name\")] attributes."
    );
}

#[test]
fn a_duckduckgo_page_yields_the_unwrapped_url_title_and_snippet() {
    let hits = parse_page(DDG, engine("duckduckgo"));
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
    let hits = parse_page(BRAVE, engine("brave"));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].url, "https://stackoverflow.com/questions/72263644");
    assert_eq!(hits[0].title, "rust - Tokio select macro");
    assert_eq!(hits[0].snippet, "Each branch <runs> concurrently.");
}

#[test]
fn freshness_maps_to_the_engine_parameter() {
    let url = engine("brave")
        .request("tokio select", Some(Window::Week))
        .unwrap();
    assert_eq!(
        url.as_str(),
        "https://search.brave.com/search?q=tokio+select&tf=pw"
    );
    let url = engine("duckduckgo")
        .request("tokio", Some(Window::Day))
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
    let error = Window::parse(Some("year")).unwrap_err();
    assert!(error.to_string().contains("not `year`"), "{error}");
    assert_eq!(Window::parse(Some(" month ")).unwrap(), Some(Window::Month));
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

#[test]
fn a_challenge_page_or_a_rate_limit_counts_as_a_blocked_engine() {
    let duckduckgo = engine("duckduckgo");
    assert!(duckduckgo.blocked(202, "<div class=\"anomaly-modal__modal\">"));
    assert!(duckduckgo.blocked(200, "<form id=\"challenge-form\" class=\"anomaly-modal\">"));
    assert!(engine("bing").blocked(429, ""));
    assert!(!duckduckgo.blocked(200, DDG));
    assert!(!engine("bing").blocked(200, BING));
}

#[test]
fn a_blocked_engine_rests_until_its_cooldown_ends() {
    use std::time::{Duration, Instant};
    let rest = super::Rest::default();
    let now = Instant::now();
    rest.block("duckduckgo", now + Duration::from_secs(60));
    assert!(rest.resting("duckduckgo", now + Duration::from_secs(59)));
    assert!(!rest.resting("duckduckgo", now + Duration::from_secs(61)));
    assert!(!rest.resting("bing", now));
    assert_eq!(engine("duckduckgo").cooldown(), Duration::from_secs(60));
}
