use super::*;

impl Work {
    fn planned_ledger(&self) -> Vec<String> {
        let Some(plan) = self.plan.as_deref() else {
            return Vec::new();
        };
        if let Ok(parsed) = crate::steps::parse(plan) {
            return parsed
                .iter()
                .take(LEDGER_MAX)
                .map(|step| format!("{}. {}", step.index, step.text))
                .collect();
        }
        plan.lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .take(LEDGER_MAX)
            .map(str::to_string)
            .collect()
    }

    pub(super) fn ledger(&self) -> Vec<String> {
        match self.policy {
            Policy::Single => self.planned_ledger(),
            Policy::Steps => self.step_ledger(),
            Policy::Goal { .. } => Vec::new(),
        }
    }

    fn step_ledger(&self) -> Vec<String> {
        let mut lines = Vec::new();
        let mut phase: Option<&str> = None;
        let mut number = 0;
        let compact = self.steps.len() > LEDGER_MAX;
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
            let title = crate::text::ellipsize(&step.title, LEDGER_WIDTH);
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

fn tally_state(lines: &mut Vec<String>, state: StepState) {
    match state {
        StepState::Completed => tally(lines, "[x]", "done"),
        StepState::Pending => tally(lines, "[ ]", "later"),
        StepState::Blocked => tally(lines, "[!]", "blocked"),
        StepState::Active => {}
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
