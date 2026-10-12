use crate::provider::{Effort, Off, Scale};

mod anthropic;
mod gemini;
mod openai;
mod vendors;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Thinking {
    Adaptive,
    Budgeted,
    EffortBudget,
    Budget,
    Level,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Family {
    Other,
    Anthropic,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Traits {
    pub(crate) family: Family,
    pub(crate) thinking: Thinking,
    pub(crate) skip_signature: bool,
    pub(crate) scale: Option<Scale>,
    pub(crate) replay: bool,
    pub(crate) preserved: bool,
    pub(crate) tool_stream: bool,
    pub(crate) tool_images: bool,
}

struct Row {
    parts: &'static [&'static str],
    traits: Traits,
}

const NONE: Traits = Traits {
    family: Family::Other,
    thinking: Thinking::None,
    skip_signature: false,
    scale: None,
    replay: false,
    preserved: false,
    tool_stream: false,
    tool_images: false,
};

const fn row(
    parts: &'static [&'static str],
    thinking: Thinking,
    levels: &'static [Effort],
    off: Off,
) -> Row {
    Row {
        parts,
        traits: Traits {
            family: Family::Other,
            thinking,
            skip_signature: false,
            scale: Some(Scale::new(levels, off)),
            replay: false,
            preserved: false,
            tool_stream: false,
            tool_images: false,
        },
    }
}

const fn signed(
    parts: &'static [&'static str],
    thinking: Thinking,
    levels: &'static [Effort],
    off: Off,
) -> Row {
    Row {
        parts,
        traits: Traits {
            family: Family::Other,
            thinking,
            skip_signature: true,
            scale: Some(Scale::new(levels, off)),
            replay: false,
            preserved: false,
            tool_stream: false,
            tool_images: true,
        },
    }
}

const fn scaled(parts: &'static [&'static str], levels: &'static [Effort], off: Off) -> Row {
    row(parts, Thinking::None, levels, off)
}

const fn claude(row: Row) -> Row {
    Row {
        parts: row.parts,
        traits: Traits {
            family: Family::Anthropic,
            ..row.traits
        },
    }
}

const fn replayed(row: Row) -> Row {
    Row {
        parts: row.parts,
        traits: Traits {
            replay: true,
            ..row.traits
        },
    }
}

const fn zai(row: Row) -> Row {
    let row = replayed(row);
    Row {
        parts: row.parts,
        traits: Traits {
            preserved: true,
            tool_stream: true,
            ..row.traits
        },
    }
}

const TABLES: [&[Row]; 4] = [anthropic::ROWS, gemini::ROWS, openai::ROWS, vendors::ROWS];

pub(crate) fn traits(model: &str) -> Traits {
    let model = model.to_ascii_lowercase();
    TABLES
        .iter()
        .flat_map(|rows| rows.iter())
        .find(|row| row.parts.iter().all(|part| model.contains(part)))
        .map_or(NONE, |row| row.traits)
}
