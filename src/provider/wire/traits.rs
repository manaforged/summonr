#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Thinking {
    Adaptive,
    Budgeted,
    Budget { can_disable: bool },
    Level { minimal: bool, medium: bool },
    None,
}

#[derive(Clone, Copy)]
pub(crate) struct Traits {
    pub(crate) thinking: Thinking,
    pub(crate) skip_signature: bool,
}

struct Row {
    parts: &'static [&'static str],
    traits: Traits,
}

const NONE: Traits = Traits {
    thinking: Thinking::None,
    skip_signature: false,
};

const fn row(parts: &'static [&'static str], thinking: Thinking) -> Row {
    Row {
        parts,
        traits: Traits {
            thinking,
            skip_signature: false,
        },
    }
}

const fn signed(parts: &'static [&'static str], thinking: Thinking) -> Row {
    Row {
        parts,
        traits: Traits {
            thinking,
            skip_signature: true,
        },
    }
}

const LEVEL_FULL: Thinking = Thinking::Level {
    minimal: true,
    medium: true,
};
const LEVEL_NO_MINIMAL: Thinking = Thinking::Level {
    minimal: false,
    medium: true,
};
const LEVEL_HIGH: Thinking = Thinking::Level {
    minimal: false,
    medium: false,
};

const ROWS: &[Row] = &[
    row(&["claude-opus-4-6"], Thinking::Adaptive),
    row(&["claude-sonnet-4-6"], Thinking::Adaptive),
    row(&["claude-opus-4-7"], Thinking::Adaptive),
    row(&["claude-opus-4-8"], Thinking::Adaptive),
    row(&["claude-opus-5"], Thinking::Adaptive),
    row(&["claude-sonnet-5"], Thinking::Adaptive),
    row(&["claude-fable-5"], Thinking::Adaptive),
    row(&["claude-mythos-5"], Thinking::Adaptive),
    row(&["claude-3"], Thinking::Budgeted),
    row(&["claude-sonnet-4-5"], Thinking::Budgeted),
    row(&["claude-opus-4-5"], Thinking::Budgeted),
    row(&["claude-haiku-4-5"], Thinking::Budgeted),
    row(&["claude-opus-4-1"], Thinking::Budgeted),
    row(&["claude-opus-4-0"], Thinking::Budgeted),
    row(&["claude-sonnet-4-0"], Thinking::Budgeted),
    row(&["claude-opus-4-2025"], Thinking::Budgeted),
    row(&["claude-sonnet-4-2025"], Thinking::Budgeted),
    row(
        &["gemini-2.5", "pro"],
        Thinking::Budget { can_disable: false },
    ),
    row(&["gemini-2.5"], Thinking::Budget { can_disable: true }),
    signed(&["gemini-3-pro"], LEVEL_HIGH),
    signed(&["gemini-3", "pro"], LEVEL_NO_MINIMAL),
    signed(&["gemini-3", "3.7"], LEVEL_NO_MINIMAL),
    signed(&["gemini-3", "3.8"], LEVEL_NO_MINIMAL),
    signed(&["gemini-3"], LEVEL_FULL),
];

pub(crate) fn traits(model: &str) -> Traits {
    ROWS.iter()
        .find(|row| row.parts.iter().all(|part| model.contains(part)))
        .map_or(NONE, |row| row.traits)
}
