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
