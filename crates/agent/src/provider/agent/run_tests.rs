#[test]
fn the_models_claude_code_announces_are_kept_as_a_sorted_plan_listing() {
    let ready = serde_json::json!({"models": [
        {"resolvedModel": "claude-sonnet-lane", "displayName": "Sonnet Lane"},
        {"resolvedModel": "claude-opus-lane"},
        {"resolvedModel": "claude-opus-lane"}
    ]});
    super::keep_listing("lane-kept-claude", &ready);
    let listing = crate::catalog::listing("lane-kept-claude").expect("the listing is kept");
    let ids: Vec<&str> = listing
        .models
        .iter()
        .map(|model| model.id.as_str())
        .collect();
    assert_eq!(ids, ["claude-opus-lane", "claude-sonnet-lane"]);
    assert!(
        listing
            .models
            .iter()
            .all(|model| model.cost == crate::catalog::Cost::Plan)
    );
    assert_eq!(listing.models[1].display.as_deref(), Some("Sonnet Lane"));
}
