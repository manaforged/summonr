use super::Model;

#[test]
fn a_december_release_keeps_its_own_year() {
    let model = Model {
        release: Some(19_722),
        ..Model::named("year-end")
    };
    assert_eq!(model.released().as_deref(), Some("2023-12"));
}

fn seat(images: Option<bool>) -> crate::Endpoint {
    let mut endpoint =
        crate::endpoint::resolve(Some("zai"), Some("glm-5.3-flash"), crate::Need::Config).unwrap();
    endpoint.name = "env".to_string();
    endpoint.base_url = Some("http://172.17.0.1:8095/zai/v1".to_string());
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
fn a_seat_behind_a_proxy_sees_images_when_the_catalog_says_its_model_does() {
    assert!(super::vision_of(&seat(None), &registry()));
}

#[test]
fn a_declared_images_row_wins_over_the_catalog() {
    assert!(!super::accepts_images(&seat(Some(false))));
}

#[test]
fn a_known_vision_model_reads_images_with_no_downloaded_catalog() {
    assert!(super::vision_of(&seat(None), &super::registry::snapshot()));
}

#[test]
fn the_catalog_keeps_only_providers_summonr_can_reach() {
    let registry = super::registry::decode(
        r#"{"zai": {"name": "Z.AI", "models": {"glm-5.3-flash": {"modalities": {"input": ["text", "image"]}}}},
            "nowhere": {"name": "Nowhere", "models": {"seer": {"modalities": {"input": ["text", "image"]}}}}}"#,
    )
    .unwrap();
    let mut elsewhere = seat(None);
    elsewhere.model = "seer".to_string();
    assert!(super::vision_of(&seat(None), &registry));
    assert!(!super::vision_of(&elsewhere, &registry));
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
    super::remember("lane-cached-seat", vec![Model::named("lane-model")]).unwrap();
    let cached = super::cached();
    let listing = cached
        .get("lane-cached-seat")
        .expect("the listing is cached");
    assert_eq!(listing.models[0].id, "lane-model");
}
