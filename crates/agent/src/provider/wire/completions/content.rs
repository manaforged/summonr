use serde::Deserialize;

#[derive(Deserialize)]
#[serde(untagged)]
pub(super) enum Content {
    Text(String),
    Chunks(Vec<Chunk>),
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum Chunk {
    Text {
        text: String,
    },
    Thinking {
        thinking: Vec<Chunk>,
    },
    #[serde(other)]
    Other,
}

impl Chunk {
    pub(super) fn text(&self) -> &str {
        match self {
            Self::Text { text } => text,
            Self::Thinking { .. } | Self::Other => "",
        }
    }
}
