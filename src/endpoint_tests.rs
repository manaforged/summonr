use super::*;

#[test]
fn zai_builtins_match_the_coding_plan_regions() {
    let global = builtin("zai").expect("global Z.ai builtin");
    assert_eq!(global.base_url.as_deref(), Some(GLM_INTL));
    assert_eq!(global.model, "glm-5.3-flash");
    assert!(
        preset("zai")
            .expect("global preset")
            .env_keys
            .contains(&"ZAI_API_KEY")
    );

    let china = builtin("zai-cn").expect("China Z.ai builtin");
    assert_eq!(china.base_url.as_deref(), Some(GLM_CN));
    assert_eq!(china.model, "glm-5.3-flash");
    assert!(
        preset("zai-cn")
            .expect("China preset")
            .env_keys
            .contains(&"ZAI_CODING_CN_API_KEY")
    );
}

#[test]
fn environment_key_scopes_are_isolated() {
    const CASE: &str = "SUMMON_ENDPOINT_ENV_CASE";
    if let Ok(case) = std::env::var(CASE) {
        match case.as_str() {
            "named" => {
                let endpoint = fill_key(builtin("grok").expect("grok preset"), false)
                    .expect("resolve named key");
                assert!(endpoint.api_key.is_none());
            }
            "generic" => {
                let endpoint = resolve_inner(None, None, true).expect("resolve generic key");
                assert_eq!(endpoint.api_key.as_deref(), Some("legacy-glm-key"));
                let changed = resolve_inner(Some(&endpoint.name), Some("alternate-model"), true)
                    .expect("change the environment endpoint model");
                assert_eq!(changed.name, endpoint.name);
                assert_eq!(changed.base_url, endpoint.base_url);
                assert_eq!(changed.api_key, endpoint.api_key);
                assert_eq!(changed.model, "alternate-model");
                assert_eq!(ready().iter().filter(|name| *name == "env").count(), 1);
                let named = resolve_inner(Some("grok"), None, false).expect("named endpoint");
                assert_eq!(named.name, "grok");
                assert_ne!(named.base_url, endpoint.base_url);
                assert!(named.api_key.is_none());
            }
            other => panic!("unknown endpoint environment test case: {other}"),
        }
        return;
    }

    let home = tempfile::tempdir().expect("temporary MAGE_HOME");
    for case in ["named", "generic"] {
        let mut child = std::process::Command::new(std::env::current_exe().expect("test binary"));
        child
            .args([
                "endpoint::tests::environment_key_scopes_are_isolated",
                "--exact",
            ])
            .env_clear()
            .env(CASE, case)
            .env("MAGE_HOME", home.path());
        if case == "named" {
            child.env("MAGE_API_KEY", "generic-key");
        } else {
            child
                .env("MAGE_BASE_URL", "https://example.invalid/v1")
                .env("GLM_API_KEY", "legacy-glm-key");
        }
        assert!(child.status().expect("run isolated key test").success());
    }
}

#[test]
fn glm_builtin_points_at_coding_plan_cn() {
    let Some(endpoint) = builtin("glm") else {
        panic!("glm builtin missing");
    };
    assert_eq!(endpoint.base_url.as_deref(), Some(GLM_CN));
    assert_eq!(endpoint.model, "glm-5.3");
}

#[test]
fn kimi_builtin_is_coding_plan() {
    let Some(endpoint) = builtin("kimi") else {
        panic!("kimi builtin missing");
    };
    assert_eq!(endpoint.base_url.as_deref(), Some(KIMI));
    assert_eq!(endpoint.model, "k3");
    assert_eq!(named_model("kimi", "kimi-k3"), "k3");
}

#[test]
fn fill_key_keeps_a_config_key() {
    let mut endpoint = builtin("grok").expect("grok");
    endpoint.api_key = Some("from-config".to_string());
    let endpoint = fill_key(endpoint, true).expect("key");
    assert_eq!(endpoint.api_key.as_deref(), Some("from-config"));
}

#[test]
fn fill_key_skips_api_key_when_oauth_is_set() {
    let mut endpoint = builtin("grok").expect("grok");
    endpoint.oauth = Some(crate::config::OAuth {
        token_url: "https://auth.example/token".into(),
        client_id: "id".into(),
        client_secret: Some("secret".into()),
        refresh_token: None,
        scope: None,
        grant: None,
        device_url: None,
        exchange_url: None,
    });
    let endpoint = fill_key(endpoint, true).expect("oauth");
    assert!(endpoint.api_key.is_none());
    assert!(endpoint.oauth.is_some());
}

#[test]
fn copilot_builtin_is_github_device_login() {
    let Some(endpoint) = builtin("copilot") else {
        panic!("copilot builtin missing");
    };
    assert_eq!(endpoint.base_url.as_deref(), Some(COPILOT));
    assert_eq!(endpoint.model, "gpt-4o");
    let oauth = endpoint.oauth.as_ref().expect("oauth");
    assert_eq!(oauth.device_url.as_deref(), Some(GITHUB_DEVICE));
    assert_eq!(oauth.token_url, GITHUB_TOKEN);
    assert_eq!(oauth.client_id, COPILOT_CLIENT);
    assert_eq!(oauth.grant.as_deref(), Some("device_code"));
    assert!(oauth.client_secret.is_none());
    assert!(
        oauth
            .exchange_url
            .as_deref()
            .is_some_and(|url| url.contains("copilot_internal"))
    );
    assert!(endpoint.headers.contains_key("Editor-Version"));
    assert!(is_named("copilot"));
    let filled = fill_key(endpoint, true).expect("oauth needs no api key");
    assert!(filled.api_key.is_none());
}

#[test]
fn kimi_missing_key_does_not_offer_device_login() {
    let row = preset("kimi").expect("kimi");
    assert!(row.oauth.is_none());
    assert!(row.missing.contains("kimi-code"));
    assert!(row.missing.contains("login"));
}

#[test]
fn auth_info_does_not_expose_keys() {
    let mut file = File::default();
    file.endpoint.insert(
        "glm".to_string(),
        FileEndpoint {
            api_key: Some("secret-that-must-not-appear".to_string()),
            ..FileEndpoint::default()
        },
    );
    let info = auth_info_from(&file, "glm", false).expect("glm auth info");
    assert!(!info.device_login);
    assert!(info.configured);
    assert!(info.setup.contains("ZAI_CODING_API_KEY"));
    assert!(info.setup.contains("summon login glm"));
    assert!(!info.setup.contains("NAME"));
    assert!(!info.setup.contains("secret-that-must-not-appear"));
    let copilot = auth_info_from(&File::default(), "copilot", false).expect("copilot auth info");
    assert!(copilot.device_login);
    assert!(copilot.setup.contains("/login copilot"));
}

#[test]
fn anthropic_and_gemini_use_native_wires() {
    let anthropic = builtin("anthropic").expect("anthropic");
    assert_eq!(anthropic.wire, Wire::Anthropic);
    assert!(
        anthropic
            .base_url
            .as_deref()
            .unwrap()
            .contains("anthropic.com")
    );
    let gemini = builtin("gemini").expect("gemini");
    assert_eq!(gemini.wire, Wire::Gemini);
    assert!(is_named("anthropic"));
    assert!(is_named("gemini"));
}

#[test]
fn openai_and_openrouter_are_key_presets() {
    assert!(is_named("openai"));
    assert!(is_named("openrouter"));
    let openai = builtin("openai").expect("openai");
    assert_eq!(
        openai.base_url.as_deref(),
        Some("https://api.openai.com/v1")
    );
}

#[test]
fn ready_names_all_resolve() {
    for name in ready() {
        resolve(Some(&name), None).unwrap_or_else(|error| panic!("{name}: {error}"));
    }
    if let Some(next) = next("qwen") {
        resolve(Some(&next), None).unwrap_or_else(|error| panic!("next qwen → {next}: {error}"));
    }
}

#[test]
fn native_catalog_uses_real_model_ids() {
    let anthropic = catalog(Wire::Anthropic);
    assert!(anthropic.iter().any(|model| model == "claude-sonnet-4-5"));
    assert!(!anthropic.iter().any(|model| model == "native"));
    let gemini = catalog(Wire::Gemini);
    assert!(gemini.iter().any(|model| model == "gemini-2.5-flash"));
    assert!(gemini.iter().any(|model| model == "gemini-2.5-pro"));
}

#[test]
fn grok_builtin_is_xai_chat() {
    let Some(endpoint) = builtin("grok") else {
        panic!("grok builtin missing");
    };
    assert_eq!(endpoint.base_url.as_deref(), Some(GROK));
    assert_eq!(endpoint.model, "grok-4.6");
    assert_eq!(named_model("grok", "grok"), "grok-4.6");
    assert_eq!(named_model("grok", "grok-4-6"), "grok-4.6");
    assert!(is_named("grok"));
}

#[test]
fn qwen_builtin_is_local_llama() {
    let Some(endpoint) = builtin("qwen") else {
        panic!("qwen builtin missing");
    };
    assert_eq!(endpoint.base_url.as_deref(), Some(QWEN_LOCAL));
    assert_eq!(endpoint.model, "qwen3.8-27b-ud");
    assert_eq!(endpoint.api_key.as_deref(), Some("local"));
}

#[test]
fn body_table_becomes_request_json() {
    let file: std::result::Result<File, _> = toml::from_str(
        r#"
[endpoint.impl]
base_url = "http://127.0.0.1:8081/v1"
model = "qwen3.8-27b-ud"
idle_timeout_secs = 600
[endpoint.impl.body]
temperature = 0.6
top_k = 20
chat_template_kwargs = { reasoning_effort = "low" }
"#,
    );
    let Ok(file) = file else {
        panic!("config parses");
    };
    let Some(endpoint) = from_file(&file, "impl") else {
        panic!("impl endpoint missing");
    };
    assert_eq!(
        endpoint.body,
        Some(serde_json::json!({
            "temperature": 0.6,
            "top_k": 20,
            "chat_template_kwargs": {"reasoning_effort": "low"}
        }))
    );
    assert_eq!(endpoint.idle_timeout_secs, Some(600));
    assert_eq!(builtin("qwen").and_then(|endpoint| endpoint.body), None);
}

#[test]
fn adopt_moves_plaintext_keys_out_of_config() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        "[endpoint.glm]\napi_key = \"sk-glm\"\n\n[endpoint.kimi]\nbase_url = \"https://kimi.invalid/v1\"\n",
    )
    .expect("write");
    let moved = adopt_from(&path).expect("adopt");
    assert_eq!(moved, vec!["glm".to_string()]);
    let text = std::fs::read_to_string(&path).expect("read");
    assert!(!text.contains("sk-glm"), "{text}");
    assert!(text.contains("kimi.invalid"), "other rows stay: {text}");
    assert_eq!(
        secret::get("glm")
            .expect("get")
            .map(|(key, _)| key)
            .as_deref(),
        Some("sk-glm")
    );
    assert!(adopt_from(&path).expect("adopt again").is_empty());
    std::fs::write(&path, "[endpoint.glm]\napi_key = \"sk-stale\"\n").expect("write");
    assert_eq!(adopt_from(&path).expect("adopt"), vec!["glm".to_string()]);
    assert_eq!(
        secret::get("glm")
            .expect("get")
            .map(|(key, _)| key)
            .as_deref(),
        Some("sk-glm"),
        "the stored key wins over a stale config row"
    );
    assert!(
        !std::fs::read_to_string(&path)
            .expect("read")
            .contains("sk-stale")
    );
    secret::forget("glm").expect("forget");
}
