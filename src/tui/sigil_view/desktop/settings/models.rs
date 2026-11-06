use sigil::prelude::{NodeBuilder, button};

use super::super::App;
use super::parts::{choice, row, section, select};
use crate::prefs::{ToolOutput, Visibility};
use crate::provider::Effort;

mod list;

const REASONING: [Visibility; 3] = [Visibility::Off, Visibility::Collapsed, Visibility::Expanded];
const TOOLS: [ToolOutput; 3] = [ToolOutput::Hidden, ToolOutput::Compact, ToolOutput::Full];

pub(super) fn view(app: &App) -> Vec<NodeBuilder> {
    if !app.picker.open.peek() {
        app.list_models(None);
    }
    vec![current(app), display(app), list::view(app)]
}

fn current(app: &App) -> NodeBuilder {
    let snapshot = app.model.peek().snapshot.clone();
    let about = if snapshot.model.is_empty() {
        "No model chosen yet".to_owned()
    } else {
        format!("{} · {}", snapshot.model, list::account(&snapshot.endpoint))
    };
    let change = app.clone();
    let mut rows = vec![row(
        "Default model",
        Some(&about),
        button("Change…")
            .accessible("Change model")
            .activate(move || change.begin_model_edit())
            .into(),
    )];
    if let Some(thinking) = thinking(app, &snapshot.levels, snapshot.effort) {
        rows.push(thinking);
    }
    section(Some("Model"), rows)
}

fn thinking(app: &App, levels: &[Effort], effort: Option<Effort>) -> Option<NodeBuilder> {
    if levels.is_empty() {
        return None;
    }
    let at = effort
        .and_then(|effort| levels.iter().position(|level| *level == effort))
        .unwrap_or(0);
    let names = levels
        .iter()
        .map(|level| effort_name(*level).to_owned())
        .collect();
    let set = app.clone();
    let levels = levels.to_vec();
    Some(select(
        "Thinking",
        Some("How long the model thinks before it answers"),
        names,
        at,
        move |index| {
            if let Some(level) = levels.get(index) {
                set.effort_command(level.as_str());
            }
        },
    ))
}

const fn effort_name(effort: Effort) -> &'static str {
    match effort {
        Effort::None => "Off",
        Effort::Minimal => "Minimal",
        Effort::Low => "Low",
        Effort::Medium => "Medium",
        Effort::High => "High",
        Effort::XHigh => "Extra high",
        Effort::Max => "Max",
    }
}

fn display(app: &App) -> NodeBuilder {
    let (shown, output) = {
        let model = app.model.peek();
        (model.prefs.reasoning, model.prefs.tools)
    };
    let reasoning = REASONING
        .iter()
        .position(|value| *value == shown)
        .unwrap_or(1);
    let tools = TOOLS.iter().position(|value| *value == output).unwrap_or(1);
    let (think, run) = (app.clone(), app.clone());
    section(
        Some("In replies"),
        vec![
            choice(
                "Reasoning",
                Some("Show the model's thinking in its replies"),
                &["Hide", "Folded", "Show"],
                reasoning,
                move |index| {
                    think
                        .set_visibility(|prefs| &mut prefs.reasoning, REASONING.get(index).copied())
                },
            ),
            choice(
                "Tool output",
                Some("How much command output each step shows"),
                &["Hide", "Short", "Full"],
                tools,
                move |index| {
                    if let Some(value) = TOOLS.get(index) {
                        run.set_tools(*value);
                    }
                },
            ),
        ],
    )
}
