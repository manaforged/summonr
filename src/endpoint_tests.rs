use super::resolve::{choose, closest, from_file, hosted};

fn row(name: &str, base_url: &str) -> super::Endpoint {
    let file: crate::config::File = toml::from_str(&format!(
        "[endpoint.{name}]\nbase_url = \"{base_url}\"\nmodel = \"m\"\n"
    ))
    .unwrap();
    from_file(&file, name).unwrap()
}

#[test]
fn a_host_on_a_private_network_needs_no_key() {
    for (name, url) in [
        ("lan", "http://192.168.1.20:8080/v1"),
        ("tailnet", "http://100.64.3.4:8081/v1"),
        ("mdns", "http://mybox.local:8080/v1"),
        ("loop", "http://[::1]:8080/v1"),
        ("docker", "http://host.docker.internal:8080/v1"),
    ] {
        assert!(row(name, url).keyless(), "{url} should need no key");
    }
}

#[test]
fn a_public_host_that_only_mentions_localhost_needs_a_key() {
    assert!(!row("fake", "https://localhost.example.com/v1").keyless());
    assert!(!row("public", "https://api.example.com/v1").keyless());
}

#[test]
fn a_custom_row_matches_the_preset_with_the_longest_base_url() {
    assert_eq!(hosted("https://opencode.ai/zen/go/v1").unwrap().name, "go");
    assert_eq!(hosted("https://opencode.ai/zen/v1").unwrap().name, "zen");
    assert!(hosted("https://opencode.ai.evil.example/zen/v1").is_none());
}

#[test]
fn a_misspelled_endpoint_suggests_the_closest_name() {
    let names = vec![
        "anthropic".to_string(),
        "openai".to_string(),
        "gemini".to_string(),
    ];
    assert_eq!(closest("antropic", &names), Some("anthropic"));
    assert_eq!(closest("zzzzzzzz", &names), None);
}

#[test]
fn the_last_seat_wins_over_the_config_default() {
    let file: crate::config::File =
        toml::from_str("default = \"qwen\"\ndefault_model = \"q\"\n").unwrap();
    let last = Some(("zai".to_string(), "glm".to_string()));
    let (name, model, reason) = choose(&file, last, None, None);
    assert_eq!(
        (name.as_deref(), model.as_deref(), reason),
        (Some("zai"), Some("glm"), Some("last used"))
    );
    let (name, _, reason) = choose(&file, None, None, None);
    assert_eq!((name.as_deref(), reason), (Some("qwen"), Some("default")));
}
