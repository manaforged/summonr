use std::path::Path;

use serde::Deserialize;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Better {
    Higher,
    Lower,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Metric {
    pub(crate) command: String,
    #[serde(default)]
    pub(crate) pattern: Option<String>,
    pub(crate) better: Better,
    #[serde(default)]
    pub(crate) target: Option<f64>,
    #[serde(default)]
    pub(crate) noise: f64,
    #[serde(default)]
    pub(crate) timeout_secs: Option<u64>,
    #[serde(default)]
    pub(crate) protect: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Protected(Vec<(String, Option<Vec<u8>>)>);

impl Protected {
    pub(crate) fn capture(workspace: &Path, paths: &[String]) -> Self {
        Self(
            paths
                .iter()
                .map(|path| (path.clone(), std::fs::read(workspace.join(path)).ok()))
                .collect(),
        )
    }

    pub(crate) fn changed(&self, workspace: &Path) -> Option<&str> {
        self.0
            .iter()
            .find(|(path, bytes)| std::fs::read(workspace.join(path)).ok() != *bytes)
            .map(|(path, _)| path.as_str())
    }
}

const NUMBER: &str = r"[-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?";

impl Metric {
    #[must_use]
    pub(crate) fn read(&self, output: &str) -> Option<(String, f64)> {
        let text = match &self.pattern {
            Some(pattern) => regex::Regex::new(pattern)
                .ok()?
                .captures_iter(output)
                .last()
                .and_then(|found| found.get(1).or_else(|| found.get(0)))?
                .as_str()
                .trim()
                .to_string(),
            None => regex::Regex::new(NUMBER)
                .ok()?
                .find_iter(output)
                .last()?
                .as_str()
                .to_string(),
        };
        let value: f64 = text.parse().ok()?;
        value.is_finite().then_some((text, value))
    }

    #[must_use]
    pub(crate) fn beats(&self, value: f64, best: Option<f64>) -> bool {
        best.is_none_or(|best| match self.better {
            Better::Higher => value > best + self.noise,
            Better::Lower => value < best - self.noise,
        })
    }

    #[must_use]
    pub(crate) fn rules(&self) -> String {
        let direction = match self.better {
            Better::Higher => "higher",
            Better::Lower => "lower",
        };
        let mut lines = vec![
            format!("Harness metric: after each turn the harness runs `{}`; {direction} is better.", self.command),
            format!("A turn is kept only when its value beats the best by more than {}. Otherwise the harness restores every file to the start of the turn.", self.noise),
            "A failed check, a missing value, or an edit to a protected file also restores the turn. The best value is held outside the workspace.".to_string(),
        ];
        if !self.protect.is_empty() {
            lines.push(format!("Protected files: {}.", self.protect.join(", ")));
        }
        if let Some(target) = self.target {
            lines.push(format!("The goal ends when the best reaches {target}."));
        }
        lines.push("Make one change per turn to the code the metric measures.".to_string());
        lines.join("\n")
    }

    #[must_use]
    pub(crate) fn reached(&self, best: Option<f64>) -> bool {
        match (self.target, best) {
            (Some(target), Some(best)) => match self.better {
                Better::Higher => best >= target,
                Better::Lower => best <= target,
            },
            _ => false,
        }
    }
}

#[cfg(test)]
#[path = "metric_tests.rs"]
mod tests;
