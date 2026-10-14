use super::pick;

fn file(text: &str) -> crate::config::File {
    toml::from_str(text).unwrap()
}

fn base(file: &crate::config::File, name: &str) -> String {
    let route = pick(file, name);
    super::super::resolve::seat(file, &route)
        .and_then(|endpoint| endpoint.base_url)
        .unwrap_or_default()
}

#[test]
fn a_provider_toggle_picks_the_route_for_its_region_and_access() {
    let china = file("[provider.zai]\nregion = \"cn\"\naccess = \"plan\"\n");
    assert_eq!(
        base(&china, "zai"),
        "https://open.bigmodel.cn/api/coding/paas/v4"
    );
    let key = file("[provider.zai]\nregion = \"intl\"\naccess = \"key\"\n");
    assert_eq!(base(&key, "zai"), "https://api.z.ai/api/paas/v4");
    let plain = file("");
    assert_eq!(base(&plain, "zai"), "https://api.z.ai/api/coding/paas/v4");
}

#[test]
fn a_route_with_a_key_wins_when_no_toggle_is_set() {
    let keyed = file("[endpoint.zai-cn]\napi_key = \"k\"\n");
    assert_eq!(pick(&keyed, "zai"), "zai-cn");
}

#[test]
fn old_row_names_still_reach_their_route_unless_the_user_owns_the_name() {
    let plain = file("");
    assert_eq!(pick(&plain, "glm"), "zai-cn");
    assert_eq!(pick(&plain, "glm-intl"), "zai-intl");
    assert_eq!(pick(&plain, "zai-cn"), "zai-cn");
    assert_eq!(pick(&plain, "zen"), "opencode-zen");
    let owned = file("[endpoint.glm]\nbase_url = \"http://127.0.0.1:9/v1\"\nmodel = \"m\"\n");
    assert_eq!(pick(&owned, "glm"), "glm");
}

#[test]
fn the_provider_list_names_each_provider_once() {
    let names = super::provider_names(&file("[endpoint.zai-cn]\nmodel = \"glm-5.3\"\n"));
    for provider in [
        "zai",
        "alibaba",
        "minimax",
        "siliconflow",
        "anthropic",
        "opencode",
        "kimi",
    ] {
        assert_eq!(
            names.iter().filter(|name| *name == provider).count(),
            1,
            "{provider} in {names:?}"
        );
    }
    for route in [
        "zai-cn",
        "glm",
        "glm-intl",
        "qwen-plan",
        "dashscope-cn",
        "minimax-cn-plan",
        "claude",
        "zen",
        "go",
    ] {
        assert!(
            !names.iter().any(|name| name == route),
            "{route} in {names:?}"
        );
    }
}

#[test]
fn a_toggle_switches_one_axis_and_keeps_the_other() {
    let mut info = super::super::auth_info("zai-cn").unwrap();
    assert_eq!(info.provider, "zai");
    info.name = "zai-cn".to_string();
    let region = super::next(&info, super::Toggle::Region).unwrap();
    assert_eq!(region.name, "zai-intl");
    let access = super::next(&info, super::Toggle::Access).unwrap();
    assert_eq!(access.name, "zai-cn-api");
    assert_eq!(info.title(), "zai");
}

#[test]
fn a_zhipu_key_runs_the_international_coding_plan() {
    let reads = |route: &str| {
        super::super::preset::preset(route)
            .unwrap()
            .env_keys
            .contains(&"ZHIPU_API_KEY")
    };
    assert!(reads("zai-intl"));
    assert!(!reads("zai-cn"));
    assert!(!reads("zai-cn-api"));
}

#[test]
fn a_key_saved_under_a_provider_lands_on_the_route_that_reads_it() {
    super::super::auth::save_key("alibaba", "sk-test-alibaba").unwrap();
    let stored = crate::secret::get("qwen-plan").unwrap();
    crate::secret::forget("qwen-plan").unwrap();
    assert!(stored.is_some(), "the key is stored for the route");
    assert!(crate::secret::get("alibaba").unwrap().is_none());
}

#[test]
fn a_key_named_for_a_route_stays_on_that_route() {
    let china = file("[provider.zai]\nregion = \"cn\"\n");
    assert_eq!(super::slot(&china, "zai-intl"), "zai-intl");
    assert_eq!(super::slot(&china, "alibaba"), "qwen-plan");
    assert_eq!(super::slot(&china, "glm"), "zai-cn");
}

#[test]
fn naming_a_route_selects_it_for_its_provider() {
    let (provider, choice) = super::choice_for("claude").unwrap();
    assert_eq!(provider, "anthropic");
    assert_eq!(choice.access, Some(super::Access::Plan));
    let (provider, choice) = super::choice_for("zai-cn").unwrap();
    assert_eq!(provider, "zai");
    assert_eq!(choice.region, Some(super::Region::Cn));
    assert!(
        super::choice_for("zai").is_none(),
        "a provider name keeps the saved choice"
    );
}

#[test]
fn a_retired_name_keeps_its_own_overrides() {
    let custom = file("[endpoint.glm]\nmodel = \"glm-5.3\"\n");
    let route = pick(&custom, "glm");
    assert_eq!(route, "zai-cn");
    let endpoint = super::super::resolve::seat(&custom, "glm").unwrap();
    assert_eq!(endpoint.model, "glm-5.3");
}

#[test]
fn no_route_shares_its_providers_name() {
    let china = file("[provider.zai]\nregion = \"cn\"\n");
    for provider in [
        "zai",
        "anthropic",
        "kimi",
        "minimax",
        "siliconflow",
        "opencode",
    ] {
        assert!(
            super::super::preset::preset(provider).is_none(),
            "{provider}"
        );
    }
    assert_eq!(super::slot(&china, "zai"), "zai-cn");
    assert_eq!(super::slot(&china, "zai-intl"), "zai-intl");
    assert_eq!(super::super::remembered("zai-cn"), "zai");
    assert_eq!(super::super::remembered("deepseek"), "deepseek");
}

#[test]
fn a_key_stored_under_an_old_route_name_still_works() {
    crate::secret::set("siliconflow", "sk-old-slot").unwrap();
    let endpoint = super::super::resolve(Some("siliconflow"), None, crate::Need::Config);
    crate::secret::forget("siliconflow").unwrap();
    let endpoint = endpoint.unwrap();
    assert_eq!(endpoint.name, "siliconflow-intl");
    assert_eq!(endpoint.api_key.as_deref(), Some("sk-old-slot"));
}

#[test]
fn a_provider_table_overlays_its_route_and_the_route_table_wins() {
    let provider = file("[endpoint.zai]\nmodel = \"glm-5.3\"\n");
    assert_eq!(
        super::super::resolve::seat(&provider, "zai").unwrap().model,
        "glm-5.3"
    );
    let both = file("[endpoint.glm]\nmodel = \"old\"\n[endpoint.zai-cn]\nmodel = \"new\"\n");
    assert_eq!(
        super::super::resolve::seat(&both, "glm").unwrap().model,
        "new"
    );
    let off = super::provider_names(&file("[endpoint.claude-pro]\nmodel = \"x\"\n"));
    assert!(!off.iter().any(|name| name == "claude-pro"), "{off:?}");
}

#[test]
fn a_provider_named_row_keeps_its_route_wire() {
    let proxy = file("[endpoint.anthropic]\nbase_url = \"https://proxy.corp/anthropic/v1\"\n");
    let route = pick(&proxy, "anthropic");
    assert_ne!(route, "anthropic", "the row took over the provider name");
    let endpoint = super::super::resolve::seat(&proxy, &route).unwrap();
    assert_eq!(endpoint.wire, crate::endpoint::Wire::Anthropic);
    let own = file(
        "[endpoint.anthropic]\nbase_url = \"https://proxy.corp/v1\"\nwire = \"completions\"\nmodel = \"m\"\n",
    );
    assert_eq!(pick(&own, "anthropic"), "anthropic");
}

#[test]
fn an_unknown_provider_key_warns_instead_of_failing() {
    let typo = file("[provider.zai]\nregon = \"cn\"\n");
    assert!(typo.provider["zai"].other.contains_key("regon"));
    assert_eq!(pick(&typo, "zai"), pick(&file(""), "zai"));
}

#[test]
fn a_signed_in_plan_route_is_ready() {
    let route = super::presets()
        .find(|preset| preset.name == "grok")
        .expect("grok route");
    assert!(
        !super::ready(&file(""), route),
        "grok is ready before any sign-in"
    );
    let slot = crate::credentials::login_slot("grok");
    crate::secret::set(&slot, "token").unwrap();
    let ready = super::ready(&file(""), route);
    crate::secret::forget(&slot).unwrap();
    assert!(ready, "a stored sign-in does not make its route ready");
}

#[test]
fn retired_names_keep_their_env_keys() {
    let route = super::super::preset::preset("zai-intl").unwrap();
    for key in [
        "ZAI_CODING_API_KEY",
        "GLM_CODING_API_KEY",
        "ZAI_API_KEY",
        "GLM_API_KEY",
    ] {
        assert!(
            route.env_keys.contains(&key),
            "zai-intl no longer reads {key}"
        );
    }
}

#[test]
fn logout_names_the_key_left_after_removal() {
    crate::secret::set("deepseek", "sk-stored").unwrap();
    let said = super::super::logout("deepseek").unwrap();
    assert!(said.starts_with("✓ Removed the stored key"), "{said}");
    assert!(!said.contains("It now uses the"), "{said}");
}

#[test]
fn the_account_environment_supplies_no_keys_to_tests() {
    assert_eq!(super::super::keys::env_key(&["PATH", "HOME"]), None);
}
