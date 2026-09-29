use crate::provider::{Effort, Off, Scale};

mod anthropic;
mod gemini;
mod openai;
mod vendors;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Thinking {
    Adaptive,
    Budgeted,
    EffortBudget,
    Budget,
    Level,
    None,
}

#[derive(Clone, Copy)]
pub(crate) struct Traits {
    pub(crate) thinking: Thinking,
    pub(crate) skip_signature: bool,
    pub(crate) scale: Option<Scale>,
}

struct Row {
    parts: &'static [&'static str],
    traits: Traits,
}

const NONE: Traits = Traits {
    thinking: Thinking::None,
    skip_signature: false,
    scale: None,
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
            thinking,
            skip_signature: false,
            scale: Some(Scale::new(levels, off)),
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
            thinking,
            skip_signature: true,
            scale: Some(Scale::new(levels, off)),
        },
    }
}

const fn scaled(parts: &'static [&'static str], levels: &'static [Effort], off: Off) -> Row {
    row(parts, Thinking::None, levels, off)
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
