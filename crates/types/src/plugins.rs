use serde::Deserialize;

/// The plugins switched off.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Switches {
    #[serde(default)]
    pub off: Vec<String>,
}
