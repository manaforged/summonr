use super::preset::Preset;
use crate::provider::{Effort, Off, Reasoning};

pub(super) fn from_preset(preset: Option<&Preset>) -> Reasoning {
    Reasoning {
        spec: preset.map(|preset| preset.reasoning).unwrap_or_default(),
        ..Reasoning::default()
    }
}

pub(super) fn overlay(reasoning: &mut Reasoning, levels: Option<&[Effort]>, off: Option<Off>) {
    if let Some(levels) = levels {
        let mut kept: Vec<Effort> = levels
            .iter()
            .copied()
            .filter(|level| *level != Effort::None)
            .collect();
        kept.sort();
        kept.dedup();
        reasoning.levels = Some(kept);
    }
    if off.is_some() {
        reasoning.off = off;
    }
}
