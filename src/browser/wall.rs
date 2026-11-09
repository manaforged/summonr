use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(crate) struct Block {
    #[serde(default)]
    statuses: Vec<u16>,
    #[serde(default)]
    markers: Vec<String>,
    body_max: Option<usize>,
}

impl Block {
    pub(crate) fn status(&self, code: u16) -> bool {
        self.statuses.contains(&code)
    }

    pub(crate) fn body(&self, shown: &str, raw: &str) -> bool {
        if self.body_max.is_some_and(|max| shown.chars().count() > max) {
            return false;
        }
        let lower = raw.to_ascii_lowercase();
        self.markers
            .iter()
            // complexity: bounded by the few block markers in engines.toml
            .any(|marker| lower.contains(marker.as_str()))
    }
}
