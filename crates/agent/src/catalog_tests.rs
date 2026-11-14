use super::Model;

#[test]
fn grok_4_7_takes_its_context_window_from_the_catalog() {
    assert_eq!(
        super::context_limit("grok", "grok-4.7", Some("https://api.x.ai/v1")),
        Some(500_000)
    );
}

#[test]
fn a_url_takes_the_preset_providers_window() {
    assert_eq!(
        super::context_limit("proxy", "grok-4.7", Some("https://api.x.ai/v1")),
        Some(500_000)
    );
    assert_eq!(
        super::context_limit(
            "copilot",
            "claude-opus-5-5",
            Some("https://api.githubcopilot.com")
        ),
        Some(1_000_000)
    );
}

#[test]
fn context_limit_does_not_inherit_another_providers_window() {
    assert_eq!(
        super::context_limit(
            "not-a-provider",
            "grok-4.7",
            Some("https://example.test/v1")
        ),
        None
    );
    assert_eq!(
        super::context_limit(
            "proxy",
            "deepseek-flash",
            Some("https://api.deepseek.com/v1")
        ),
        Some(1_000_000)
    );
    assert_eq!(
        super::context_limit("proxy", "gpt-6-astra", Some("https://api.x.ai/v1")),
        None
    );
    assert_eq!(
        super::context_limit("grok", "no-such-model", Some("https://api.x.ai/v1")),
        None
    );
}

#[test]
fn a_december_release_keeps_its_own_year() {
    let model = Model {
        release: Some(19_722),
        ..Model::named("year-end")
    };
    assert_eq!(model.released().as_deref(), Some("2023-12"));
}

fn selection(images: Option<bool>) -> crate::Endpoint {
    let mut endpoint = crate::endpoint::resolve(
        Some("zai"),
        Some("glm-5.3-flash"),
        crate::Requirement::Config,
    )
    .unwrap();
    endpoint.name = "env".to_string();
    endpoint.base_url = Some("http://127.0.0.1:8095/zai/v1".to_string());
    endpoint.images = images;
    endpoint
}

fn registry() -> super::registry::Registry {
    serde_json::from_value(serde_json::json!({
        "schema": 2,
        "providers": [{
            "id": "zai",
            "name": "Z.AI",
            "api": "https://api.z.ai/api/coding/paas/v4",
            "models": {"glm-5.3-flash": {"vision": true}}
        }]
    }))
    .unwrap()
}

#[test]
fn a_model_behind_a_proxy_sees_images_when_the_catalog_says_its_model_does() {
    assert!(super::registry::vision_of(
        super::registry::Source::Cached(&registry()),
        &selection(None)
    ));
}

#[test]
fn a_declared_images_row_wins_over_the_catalog() {
    assert!(!super::accepts_images(&selection(Some(false))));
}

#[test]
fn a_known_vision_model_reads_images_with_no_downloaded_catalog() {
    assert!(super::registry::vision_of(
        super::registry::Source::Bundled,
        &selection(None)
    ));
    let mut models = [Model::named("claude-opus-5-5")];
    super::registry::enrich_from(
        super::registry::Source::Bundled,
        "anthropic",
        None,
        &mut models,
    );
    assert_eq!(models[0].display.as_deref(), Some("Claude Opus 5.5"));
    assert_eq!(models[0].context, Some(1_000_000));
    assert_eq!(models[0].family.as_deref(), Some("claude-opus"));
    assert!(models[0].vision && models[0].reasoning);
    assert_eq!(models[0].cost, super::Cost::Metered);
    let price = models[0].price.expect("bundled model price");
    assert_eq!(
        (price.input, price.output, price.cached, price.cache_write),
        (4.0, 20.0, 0.2, Some(5.0))
    );
}

#[test]
fn the_catalog_keeps_only_providers_summonr_can_reach() {
    let registry = super::registry::decode(
        r#"{"zai": {"name": "Z.AI", "models": {"glm-5.3-flash": {"modalities": {"input": ["text", "image"]}}}},
            "nowhere": {"name": "Nowhere", "models": {"seer": {"modalities": {"input": ["text", "image"]}}}}}"#,
    )
    .unwrap();
    let mut elsewhere = selection(None);
    elsewhere.model = "seer".to_string();
    let source = super::registry::Source::Cached(&registry);
    assert!(super::registry::vision_of(source, &selection(None)));
    assert!(!super::registry::vision_of(source, &elsewhere));
}

#[test]
fn a_listing_with_top_level_input_modalities_marks_vision() {
    let models = super::parse(
        &serde_json::json!({"data": [{"id": "deepseek-flash", "input_modalities": ["text", "image"]}]}),
        super::Cost::Unknown,
    );
    assert!(models[0].vision);
}

#[test]
fn a_listing_without_a_cache_read_rate_prices_cached_tokens_as_input() {
    let models = super::parse(
        &serde_json::json!({"data": [{"id": "m", "pricing": {"prompt": "0.000003", "completion": "0.000015"}}]}),
        super::Cost::Unknown,
    );
    let price = models[0].price.expect("the listing is priced");
    assert!((price.cached - price.input).abs() < 1e-9, "{price:?}");
    assert!(price.input > 0.0);
}

#[test]
fn a_model_the_plan_hides_is_not_listed() {
    let value = serde_json::json!({"data": [
        {"id": "gpt-6.1-sol", "visibility": "list"},
        {"id": "gpt-internal", "visibility": "hidden"},
        {"id": "gpt-4o"}
    ]});
    let ids = |value: &serde_json::Value| -> Vec<String> {
        super::listing::parse(value, super::Cost::Plan)
            .into_iter()
            .map(|model| model.id)
            .collect()
    };
    assert_eq!(
        ids(&value),
        ["gpt-6.1-sol"],
        "a curated listing keeps only listed rows"
    );
    let other = serde_json::json!({"data": [
        {"id": "a", "visibility": "public"},
        {"id": "b"}
    ]});
    assert_eq!(
        ids(&other),
        ["a", "b"],
        "another host's visibility words are not a filter"
    );
}

#[test]
fn a_plan_listing_names_models_by_slug() {
    let value = serde_json::json!({"models": [
        {"slug": "gpt-6.1-sol", "display_name": "GPT-6.1 Sol", "visibility": "list"}
    ]});
    let rows = super::listing::parse(&value, super::Cost::Plan);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "gpt-6.1-sol");
    assert_eq!(rows[0].display.as_deref(), Some("GPT-6.1 Sol"));
}

#[test]
fn a_claude_plan_offers_models_without_a_listing() {
    let offered = super::registry::offered("claude", super::Cost::Plan);
    assert!(!offered.is_empty());
    assert!(
        offered.iter().all(|model| model.id.starts_with("claude-")),
        "{:?}",
        offered.iter().map(|model| &model.id).collect::<Vec<_>>()
    );
}

#[test]
fn a_remembered_listing_is_in_the_cached_listings() {
    super::remember("lane-cached-selection", vec![Model::named("lane-model")]).unwrap();
    let cached = super::cached();
    let listing = cached
        .get("lane-cached-selection")
        .expect("the listing is cached");
    assert_eq!(listing.models[0].id, "lane-model");
}
