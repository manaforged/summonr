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
