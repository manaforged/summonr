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
        ("lan10", "http://10.0.0.0:8080/v1"),
        ("lan172", "http://172.16.0.0:8080/v1"),
        ("lan192", "http://192.168.0.0:8080/v1"),
        ("tailnet", "http://100.64.0.0:8081/v1"),
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
        hosted("https://opencode.ai/inference/v1").unwrap().name,
        "opencode-zen"
    );
    assert!(hosted("https://opencode.ai.evil.example/inference/v1").is_none());
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
fn the_last_model_wins_over_the_config_default_and_a_stale_one_falls_back() {
    let servers = "[endpoint.lab]\nbase_url = \"http://127.0.0.1:9/v1\"\nmodel = \"m\"\n\n[endpoint.other]\nbase_url = \"http://127.0.0.1:9/v1\"\nmodel = \"o\"\n";
    let file: crate::config::File = toml::from_str(&format!(
        "default = \"lab\"\ndefault_model = \"m\"\n{servers}"
    ))
    .unwrap();
    let last = Some(("other".to_string(), "o".to_string()));
    let (name, model, reason) = choose(&file, last, None, None);
    assert_eq!(
        (name.as_deref(), model.as_deref(), reason),
        (Some("other"), Some("o"), Some(super::Pick::LastUsed))
    );
    let gone = Some(("gone".to_string(), "g".to_string()));
    let (name, model, reason) = choose(&file, gone, None, None);
    assert_eq!(
        (name.as_deref(), model.as_deref(), reason),
        (Some("lab"), Some("m"), Some(super::Pick::Default))
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
        for model in std::iter::once(&preset.model).chain(preset.login_model.as_ref()) {
            if let Some(current) = crate::catalog::serves(&preset.opencode_ids, model) {
                assert!(
                    current,
                    "{} defaults to {model}, which its catalog providers do not list as current",
                    preset.name
                );
            }
        }
    }
}

#[test]
fn another_tools_credential_files_are_never_read() {
    let home = summonr_core::dirs::user_home().expect("test home");
    let opencode = home.join(".local/share/opencode");
    std::fs::create_dir_all(&opencode).unwrap();
    std::fs::write(
        opencode.join("auth.json"),
        r#"{"openrouter":{"type":"api","key":"sk-or-from-opencode"},"kimi-code-plan-global":{"type":"api","key":"kimi-from-opencode"}}"#,
    )
    .unwrap();
    let kimi = home.join(".kimi-code/credentials");
    std::fs::create_dir_all(&kimi).unwrap();
    std::fs::write(
        kimi.join("kimi-code.json"),
        r#"{"access_token":"kimi-from-cli","expires_at":9999999999}"#,
    )
    .unwrap();
    for name in ["openrouter", "kimi-intl"] {
        let endpoint =
            super::keys::fill_key(super::keys::builtin(name).expect("built-in"), false).unwrap();
        let key = endpoint
            .api_key
            .map(|key| key.expose().to_string())
            .unwrap_or_default();
        assert!(
            !key.contains("from-opencode") && !key.contains("from-cli"),
            "{name} took another tool's key: {key}"
        );
    }
}

#[test]
fn an_explicit_effort_reaches_the_provider_unchanged() {
    use crate::provider::Effort;
    let endpoint = super::resolve(Some("deepseek"), None, crate::Need::Config).unwrap();
    assert_eq!(endpoint.effort_for(Some(Effort::None)), Some(Effort::None));
    assert_eq!(
        endpoint.effort_for(Some(Effort::Medium)),
        Some(Effort::Medium)
    );
}

#[test]
fn a_preset_row_keeps_its_configured_idle_timeout() {
    let file: crate::config::File =
        toml::from_str("[endpoint.grok]\nidle_timeout_secs = 7\n").unwrap();
    let endpoint = super::resolve::selection(&file, "grok").unwrap();
    assert_eq!(endpoint.idle_timeout_secs, Some(7));
}

#[test]
fn a_built_in_login_with_a_misspelled_grant_is_rejected() {
    let table = "[[preset]]\nname = \"x\"\nbase_url = \"https://x.example/v1\"\noauth = \"lab\"\n\n[oauth.lab]\ntoken_url = \"https://x.example/token\"\nclient_id = \"id\"\nlabel = \"Lab\"\ngrant = \"devce_code\"\n";
    let Err(error) = super::preset::table(table) else {
        panic!("a misspelled grant must not load");
    };
    assert!(error.contains("devce_code"), "got {error}");
}
