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
fn a_custom_row_can_point_model_listings_elsewhere() {
    let file: crate::config::File = toml::from_str(
        "[endpoint.odd]\nbase_url = \"https://odd.example\"\nmodels_url = \"https://odd.example/v1\"\nmodel = \"m\"\n",
    )
    .unwrap();
    let endpoint = from_file(&file, "odd").unwrap();
    assert_eq!(endpoint.base_url.as_deref(), Some("https://odd.example"));
    assert_eq!(
        endpoint.models_url.as_deref(),
        Some("https://odd.example/v1")
    );
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
        (Some("zai"), Some("glm"), Some(super::Pick::LastUsed))
    );
    let (name, _, reason) = choose(&file, None, None, None);
    assert_eq!(
        (name.as_deref(), reason),
        (Some("qwen"), Some(super::Pick::Default))
    );
}

#[test]
fn openai_and_grok_offer_a_sign_in_and_a_key() {
    for (name, grant) in [
        ("openai", summonr_auth::Grant::AuthorizationCode),
        ("grok", summonr_auth::Grant::DeviceCode),
    ] {
        let row = super::keys::builtin(name).unwrap();
        let spec = crate::credentials::spec(row.oauth.as_ref().unwrap()).unwrap();
        assert_eq!(spec.grant, grant, "{name}");
        assert!(row.takes_key(), "{name} still takes a key");
    }
    assert_eq!(
        super::keys::builtin("openai").unwrap().wire,
        super::Wire::Responses
    );
}

#[test]
fn the_provider_table_rejects_rows_it_cannot_honor() {
    let base = "[[preset]]\nname = \"x\"\nbase_url = \"https://x.example/v1\"\n";
    for (label, broken) in [
        ("unknown login", format!("{base}oauth = \"nope\"\n")),
        ("unknown wire", format!("{base}wire = \"carrier-pigeon\"\n")),
        (
            "duplicate row",
            format!("{base}\n[[preset]]\nname = \"x\"\nbase_url = \"https://y.example/v1\"\n"),
        ),
        (
            "lowercase env key",
            format!("{base}env_keys = [\"x_api_key\"]\n"),
        ),
        (
            "unknown effort field",
            format!("{base}[preset.reasoning]\nfield = \"vibes\"\n"),
        ),
    ] {
        assert!(
            super::preset::table(&broken).is_err(),
            "{label} should be rejected"
        );
    }
}

#[test]
fn an_off_row_is_listed_but_hidden() {
    assert!(super::preset::preset("claude-pro").is_none());
    assert!(!super::preset::presets().any(|row| row.name == "claude-pro"));
    assert!(
        super::preset::listed()
            .iter()
            .any(|row| row.name == "claude-pro" && row.off),
        "claude-pro stays in the table with off = true"
    );
}

#[test]
fn coding_plans_join_the_plan_group() {
    for name in [
        "qwen-plan",
        "qwen-plan-cn",
        "minimax-plan",
        "minimax-cn-plan",
        "tencent-plan",
        "volcengine-plan",
    ] {
        let endpoint = super::keys::builtin(name).unwrap_or_else(|| panic!("{name} missing"));
        assert!(
            matches!(super::login_group(name, &endpoint), super::LoginGroup::Plan),
            "{name} should sit in the coding-plan group"
        );
    }
}

#[test]
fn the_new_endpoints_point_at_their_hosts() {
    let qwen = super::keys::builtin("qwen-plan").unwrap();
    assert_eq!(
        qwen.base_url.as_deref(),
        Some("https://coding-intl.dashscope.aliyuncs.com/v1")
    );
    assert_eq!(qwen.wire, super::Wire::Completions);
    let minimax = super::keys::builtin("minimax-plan").unwrap();
    assert_eq!(
        minimax.base_url.as_deref(),
        Some("https://api.minimax.io/anthropic/v1")
    );
    assert_eq!(minimax.wire, super::Wire::Anthropic);
    let perplexity = super::keys::builtin("perplexity").unwrap();
    assert_eq!(
        perplexity.base_url.as_deref(),
        Some("https://api.perplexity.ai")
    );
    assert_eq!(
        perplexity.models_url.as_deref(),
        Some("https://api.perplexity.ai/v1"),
        "perplexity serves its model list under /v1 and chat without it"
    );
    let lmstudio = super::keys::builtin("lmstudio").unwrap();
    assert!(lmstudio.keyless(), "lmstudio is a local server");
    assert!(super::keys::builtin("ollama-cloud").is_some());
}

#[test]
fn opencode_peer_ids_follow_the_table() {
    assert_eq!(
        super::preset::provider_ids("qwen-plan"),
        ["alibaba-coding-plan"]
    );
    assert_eq!(
        super::preset::provider_ids("volcengine-plan"),
        ["volcengine-coding-plan"]
    );
    assert!(super::preset::catalog_providers().any(|known| known == "tencent-coding-plan"));
}

#[test]
fn hosts_that_route_caches_per_conversation_opt_into_the_cache_key() {
    assert!(
        hosted("https://api.x.ai/v1")
            .expect("xai preset")
            .prompt_cache_key,
        "xai stores cache entries per server and routes on the cache key"
    );
}
