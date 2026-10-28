use super::*;

impl Work {
    fn planned_ledger(&self) -> Vec<String> {
        let Some(plan) = self.plan.as_deref() else {
            return Vec::new();
        };
        if let Ok(parsed) = crate::steps::parse(plan) {
            return parsed
                .iter()
                .take(crate::limits::limits().ledger_max)
                .map(|step| format!("{}. {}", step.index, step.text))
                .collect();
        }
        plan.lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .take(crate::limits::limits().ledger_max)
            .map(str::to_string)
            .collect()
    }

    pub(super) fn ledger(&self) -> Vec<String> {
        match self.policy {
            WorkKind::Single => self.planned_ledger(),
            WorkKind::Steps => self.step_ledger(),
            WorkKind::Goal => self.measurement_ledger(),
        }
    }

    fn measurement_ledger(&self) -> Vec<String> {
        let skip = self
            .measurements
            .len()
            .saturating_sub(crate::limits::limits().ledger_max);
        self.measurements
            .iter()
            .enumerate()
            .skip(skip)
            .map(|(index, measurement)| {
                let outcome = match (index, measurement.kept) {
                    (0, _) => "baseline",
                    (_, true) => "kept",
                    (_, false) => "reverted",
                };
                format!(
                    "#{} {} {outcome}",
                    index + 1,
                    measurement.value.as_deref().unwrap_or("no value")
                )
            })
            .collect()
    }

    fn step_ledger(&self) -> Vec<String> {
        let mut lines = Vec::new();
        let mut phase: Option<&str> = None;
        let mut number = 0;
        let compact = self.steps.len() > crate::limits::limits().ledger_max;
        for step in &self.steps {
            if step.phase.as_deref() != phase && step.phase.is_some() {
                phase = step.phase.as_deref();
                number += 1;
                let heading: String = phase
                    .unwrap_or_default()
                    .chars()
                    .take(LEDGER_WIDTH)
                    .collect();
                lines.push(format!("{}. {heading}", roman(number)));
            }
            if compact && step.state != StepState::Active {
                tally_state(&mut lines, step.state);
                continue;
            }
            let indent = if phase.is_some() { "  " } else { "" };
            let title = crate::display::ellipsize(&step.title, LEDGER_WIDTH);
            lines.push(format!("{indent}{} {title}", step_mark(step.state)));
        }
        lines
    }
}

fn step_mark(state: StepState) -> &'static str {
    match state {
        StepState::Pending => "[ ]",
        StepState::Active => "[/]",
        StepState::Completed => "[x]",
        StepState::Blocked => "[!]",
    }
}

const LEGEND: [(StepState, &str); 4] = [
    (StepState::Completed, "done"),
    (StepState::Active, "this step"),
    (StepState::Pending, "later"),
    (StepState::Blocked, "blocked"),
];

impl Work {
    pub(crate) fn ledger_legend(kind: WorkKind) -> String {
        if kind == WorkKind::Goal {
            return "Harness metric ledger (attempt, value, outcome; #1 is the unchanged workspace, and only kept changes are in the workspace):".to_string();
        }
        let marks: Vec<String> = LEGEND
            .iter()
            .map(|(state, word)| format!("{} {word}", step_mark(*state)))
            .collect();
        format!("Harness ledger ({}):", marks.join(", "))
    }
}

fn tally_state(lines: &mut Vec<String>, state: StepState) {
    if state == StepState::Active {
        return;
    }
    if let Some((_, word)) = LEGEND.iter().find(|(listed, _)| *listed == state) {
        tally(lines, step_mark(state), word);
    }
}

fn tally(lines: &mut Vec<String>, mark: &str, word: &str) {
    let suffix = format!(" {word}");
    match lines.last_mut() {
        Some(line) if line.starts_with(&format!("  {mark} ")) && line.ends_with(&suffix) => {
            let count: usize = line
                .split_whitespace()
                .nth(2)
                .and_then(|n| n.parse().ok())
                .unwrap_or(1);
            *line = format!("  {mark} {} {word}", count + 1);
        }
        _ => lines.push(format!("  {mark} 1 {word}")),
    }
}
