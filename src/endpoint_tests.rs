use super::resolve::{choose, closest, from_file, hosted};
use super::{AuthInfo, LoginFlow, LoginGroup, Source};

fn row(name: &str, base_url: &str) -> super::Endpoint {
    let file: crate::config::File = toml::from_str(&format!(
        "[endpoint.{name}]\nbase_url = \"{base_url}\"\nmodel = \"m\"\n"
    ))
    .unwrap();
    from_file(&file, name).unwrap()
}

#[test]
fn enter_label_names_what_enter_does_per_provider_state() {
    let pinned = info(
        true,
        Some(Source::Env),
        Some("ANTHROPIC_API_KEY"),
        LoginFlow::Key,
    );
    assert_eq!(pinned.key_action(), "unset the env key");
    assert!(!pinned.paste_key());
    let stored = info(true, Some(Source::File), None, LoginFlow::Key);
    assert_eq!(stored.key_action(), "replace the key");
    assert!(stored.paste_key());
    let peer = info(true, Some(Source::Peer("opencode")), None, LoginFlow::Key);
    assert_eq!(peer.key_action(), "set a key");
    assert!(peer.paste_key());
    let missing = info(true, None, None, LoginFlow::Key);
    assert_eq!(missing.key_action(), "set a key");
    assert!(missing.paste_key());
    let plan = info(true, Some(Source::Keychain), None, LoginFlow::Browser);
    assert_eq!(plan.key_action(), "sign in");
    assert!(plan.paste_key());
}

#[test]
fn paste_key_offer_follows_the_key_and_env_fields() {
    assert!(info(true, None, None, LoginFlow::Key).paste_key());
    assert!(!info(false, None, None, LoginFlow::Key).paste_key());
    assert!(!info(true, None, Some("OPENAI_API_KEY"), LoginFlow::Key).paste_key());
}

fn info(
    key: bool,
    source: Option<Source>,
    env: Option<&'static str>,
    login: LoginFlow,
) -> AuthInfo {
    AuthInfo {
        provider: String::new(),
        routes: Vec::new(),
        name: "lab".to_string(),
        flow: LoginFlow::Key,
        login,
        key,
        plan: String::new(),
        configured: source.is_some(),
        source,
        setup: String::new(),
        env,
        model: "m".to_string(),
        base_url: None,
        own: false,
        env_hint: None,
        group: LoginGroup::Key,
    }
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
    assert_eq!(
        hosted("https://opencode.ai/zen/v1").unwrap().name,
        "opencode"
    );
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
    assert_eq!(
        closest("antropic", &names, super::Near::Typo),
        Some("anthropic")
    );
    assert_eq!(closest("zzzzzzzz", &names, super::Near::Typo), None);
}

#[test]
fn the_config_default_wins_over_the_last_seat_and_a_stale_one_falls_back() {
    let servers = "[endpoint.lab]\nbase_url = \"http://127.0.0.1:9/v1\"\nmodel = \"m\"\n\n[endpoint.other]\nbase_url = \"http://127.0.0.1:9/v1\"\nmodel = \"o\"\n";
    let last = Some(("other".to_string(), "o".to_string()));
    let file: crate::config::File = toml::from_str(&format!(
        "default = \"lab\"\ndefault_model = \"m\"\n{servers}"
    ))
    .unwrap();
    let (name, _, reason) = choose(&file, last.clone(), None, None);
    assert_eq!(
        (name.as_deref(), reason),
        (Some("lab"), Some(super::Pick::Default))
    );
    let stale: crate::config::File =
        toml::from_str(&format!("default = \"gone\"\n{servers}")).unwrap();
    let (name, _, reason) = choose(&stale, last, None, None);
    assert_eq!(
        (name.as_deref(), reason),
        (Some("other"), Some(super::Pick::LastUsed))
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
    assert!(
        super::keys::builtin("perplexity").is_none(),
        "perplexity runs no user tools"
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

#[test]
fn a_short_name_gets_a_hint_only_for_a_near_miss() {
    let names = vec!["zai".to_string(), "ollama".to_string(), "qwen".to_string()];
    assert_eq!(closest("lab", &names, super::Near::Typo), None);
    assert_eq!(closest("ollma", &names, super::Near::Typo), Some("ollama"));
    assert_eq!(closest("qwn", &names, super::Near::Typo), Some("qwen"));
    let lab = vec!["lab".to_string()];
    assert_eq!(closest("lba", &lab, super::Near::Typo), Some("lab"));
    let mine = vec!["zai".to_string(), "mylab".to_string()];
    assert_eq!(closest("lab", &mine, super::Near::Typo), Some("mylab"));
    assert_eq!(closest("lab", &mine, super::Near::Relative), None);
}

#[test]
fn a_key_row_with_a_plan_sign_in_lists_under_coding_plans_by_its_plan_name() {
    for (name, plan) in [("openai", "ChatGPT"), ("grok", "SuperGrok")] {
        let endpoint = super::keys::builtin(name).unwrap();
        let info = super::AuthInfo {
            provider: name.to_string(),
            routes: Vec::new(),
            name: name.to_string(),
            flow: super::LoginFlow::default(),
            login: endpoint
                .oauth
                .as_ref()
                .map(crate::credentials::flow)
                .unwrap_or_default(),
            key: endpoint.takes_key(),
            plan: endpoint
                .oauth
                .as_ref()
                .and_then(|spec| spec.label.clone())
                .unwrap_or_default(),
            configured: false,
            source: None,
            setup: String::new(),
            env: None,
            model: endpoint.model.clone(),
            base_url: endpoint.base_url.clone(),
            own: false,
            env_hint: None,
            group: super::login_group(name, &endpoint),
        };
        assert_eq!(info.catalog_title(), "Coding plans", "{name}");
        assert_eq!(
            info.title(),
            name,
            "the plan belongs in the notes, not the title"
        );
        assert!(
            info.matches(&plan.to_lowercase()),
            "{name} is found by its plan"
        );
        assert!(info.catalog_line(16, 16).contains(plan), "{name}");
        assert!(!info.plan(), "{name} keeps key pricing");
    }
}

#[test]
fn every_default_model_is_current_in_the_bundled_catalog() {
    for preset in super::preset::presets() {
        if preset.model.is_empty() || preset.model == "default" {
            continue;
        }
        if let Some(current) = crate::catalog::serves(preset.opencode_ids, &preset.model) {
            assert!(
                current,
                "{} defaults to {}, which its catalog providers do not list as current",
                preset.name, preset.model
            );
        }
    }
}
