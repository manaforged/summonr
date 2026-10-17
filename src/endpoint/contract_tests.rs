const ANSWERED: [u16; 5] = [200, 400, 401, 403, 429];

fn hosted(base: &str) -> bool {
    url::Url::parse(base).is_ok_and(|url| {
        matches!(url.scheme(), "http" | "https")
            && !matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))
    })
}

#[tokio::test]
#[ignore = "live: models.dev and every provider host"]
async fn every_preset_row_matches_the_live_catalog_and_its_host_answers() {
    let serves = crate::catalog::serves_live()
        .await
        .expect("models.dev catalog");
    let http = crate::credentials::http().expect("http session");
    let mut broken = Vec::new();
    for preset in super::preset::presets().filter(|preset| !preset.off) {
        let models = std::iter::once(&preset.model)
            .chain(preset.login_model.as_ref())
            .filter(|model| !model.is_empty() && model.as_str() != "default");
        for model in models {
            if serves(preset.opencode_ids, model) == Some(false) {
                broken.push(format!(
                    "{}: models.dev does not list {model} as current",
                    preset.name
                ));
            }
        }
        if !hosted(&preset.base_url) {
            continue;
        }
        let url = preset.models_url.map_or_else(
            || format!("{}/models", preset.base_url.trim_end_matches('/')),
            str::to_string,
        );
        match http.get(&url).send().await {
            Ok(response) if ANSWERED.contains(&response.status().as_u16()) => {}
            Ok(response) => broken.push(format!(
                "{}: GET {url} answered {}",
                preset.name,
                response.status().as_u16()
            )),
            Err(error) => broken.push(format!("{}: GET {url} failed: {error}", preset.name)),
        }
    }
    assert!(broken.is_empty(), "{}", broken.join("\n"));
}
