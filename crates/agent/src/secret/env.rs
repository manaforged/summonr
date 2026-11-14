use std::collections::HashSet;
use std::sync::OnceLock;

#[derive(serde::Deserialize)]
struct Extra {
    names: Vec<String>,
}

#[derive(serde::Deserialize)]
struct Presets {
    preset: Vec<Keys>,
}

#[derive(serde::Deserialize)]
struct Keys {
    #[serde(default)]
    env_keys: Vec<String>,
    #[serde(default)]
    env_unset: Vec<String>,
}

/// Whether `name` is an environment variable that carries a provider key or another credential.
pub(crate) fn credential_env(name: &str) -> bool {
    static NAMES: OnceLock<HashSet<String>> = OnceLock::new();
    NAMES
        .get_or_init(|| {
            let presets: Presets = summonr_core::embedded!("provider", "endpoint/presets.toml");
            let extra: Extra = summonr_core::embedded!("credential variable", "secret/env.toml");
            presets
                .preset
                .into_iter()
                .flat_map(|keys| keys.env_keys.into_iter().chain(keys.env_unset))
                .chain(extra.names)
                .collect()
        })
        .contains(name)
}
