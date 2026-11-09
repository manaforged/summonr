use serde::Deserialize;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Switches {
    #[serde(default)]
    pub off: Vec<String>,
}
