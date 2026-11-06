use crate::ui::{Hints, Key};
use summonr_agent::mcp::{Field, FieldKind};
use summonr_agent::prefs::Hotkeys;

pub(super) fn facts(field: &Field) -> String {
    let mut parts = vec![keys(&field.kind), hint(&field.kind)];
    if field.required {
        parts.push("required".to_string());
    }
    if let Some(default) = &field.default {
        parts.push(format!("default {}", Field::shown(default)));
    }
    parts.retain(|part| !part.is_empty());
    parts.join(" · ")
}

fn hint(kind: &FieldKind) -> String {
    match kind {
        FieldKind::Text { format, .. } => {
            format.map_or_else(String::new, |format| format.label().to_string())
        }
        FieldKind::Number {
            integer,
            minimum,
            maximum,
        } => number_hint(*integer, *minimum, *maximum),
        FieldKind::Boolean => "yes or no".to_string(),
        FieldKind::Choice { .. } | FieldKind::Choices { .. } => String::new(),
    }
}

fn keys(kind: &FieldKind) -> String {
    match kind {
        FieldKind::Choice { custom, .. } => picking(
            Hints::new()
                .key(Key::UpDown, "move")
                .key(Key::Enter, "picks"),
            *custom,
        ),
        FieldKind::Choices { custom, .. } => picking(
            Hints::new()
                .key(Key::UpDown, "move")
                .key(Key::Space, "toggles")
                .key(Key::Enter, "confirms"),
            *custom,
        ),
        FieldKind::Text { .. } | FieldKind::Number { .. } | FieldKind::Boolean => {
            picking(Hints::new().key(Key::Enter, "next"), false)
        }
    }
}

fn picking(hints: Hints, custom: bool) -> String {
    let keys = hints.key(Key::Esc, "cancel").text(&Hotkeys::default());
    if custom {
        format!("{keys} · or type your own answer")
    } else {
        keys
    }
}

fn number_hint(integer: bool, minimum: Option<f64>, maximum: Option<f64>) -> String {
    let noun = if integer { "whole number" } else { "number" };
    match (minimum, maximum) {
        (Some(low), Some(high)) => format!("{noun} {low} to {high}"),
        (Some(low), None) => format!("{noun} from {low}"),
        (None, Some(high)) => format!("{noun} up to {high}"),
        (None, None) => noun.to_string(),
    }
}
