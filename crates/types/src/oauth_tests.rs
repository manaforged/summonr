use super::*;

#[test]
fn a_debug_print_of_a_login_shows_no_client_secret_or_refresh_token() {
    let login: OAuth = toml::from_str(
        "token_url = \"https://x.example/token\"\nclient_id = \"id\"\nclient_secret = \"cs-value\"\nrefresh_token = \"rt-value\"\n",
    )
    .unwrap();
    let shown = format!("{login:?}");
    assert!(!shown.contains("cs-value"), "{shown}");
    assert!(!shown.contains("rt-value"), "{shown}");
}
