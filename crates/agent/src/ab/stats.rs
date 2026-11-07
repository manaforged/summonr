use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

/// How many bootstrap resamples each interval draws.
const RESAMPLES: usize = 2000;
/// A fixed seed, so one set of runs always reports one interval.
const SEED: u64 = 0x5eed_ab;
/// The set every other set is compared with when it is an arm.
const NONE: &str = "none";

/// One goal run under one skill set.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub(super) struct Run {
    pub set: String,
    pub round: usize,
    pub code: Option<i32>,
    pub passed: bool,
    pub tokens: u64,
    pub cost: Option<f64>,
    pub secs: f64,
    pub skills: Vec<String>,
    pub session: Option<String>,
}

/// The wire `type` of a JSON event that a run's measures come from.
enum Kind {
    Session,
    Usage,
    ToolCall,
    Child,
    Other,
}

impl Kind {
    fn of(event: &Value) -> Self {
        match event["type"].as_str() {
            Some("session") => Self::Session,
            Some("usage") => Self::Usage,
            Some("tool_call") => Self::ToolCall,
            Some("child") => Self::Child,
            _ => Self::Other,
        }
    }
}

impl Run {
    /// A run's measures from the JSON events it printed, one per line.
    pub(super) fn read(set: &str, round: usize, code: Option<i32>, secs: f64, log: &str) -> Self {
        let mut run = Self {
            set: set.to_owned(),
            round,
            code,
            passed: code == Some(0),
            secs,
            ..Self::default()
        };
        for event in log
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
        {
            run.take(&event);
        }
        run
    }

    fn take(&mut self, event: &Value) {
        match Kind::of(event) {
            Kind::Child => self.take(&event["event"]),
            Kind::Session if self.session.is_none() => {
                self.session = event["id"].as_str().map(str::to_owned);
            }
            Kind::Usage => {
                self.tokens += event["prompt_tokens"].as_u64().unwrap_or(0)
                    + event["completion_tokens"].as_u64().unwrap_or(0);
                if let Some(cost) = event["cost"].as_f64() {
                    self.cost = Some(self.cost.unwrap_or(0.0) + cost);
                }
            }
            Kind::ToolCall if event["name"] == "skill" => {
                let name = event["arguments"]
                    .as_str()
                    .and_then(|text| serde_json::from_str::<Value>(text).ok())
                    .and_then(|args| args["name"].as_str().map(str::to_owned));
                if let Some(name) = name.filter(|name| !self.skills.contains(name)) {
                    self.skills.push(name);
                }
            }
            Kind::Session | Kind::ToolCall | Kind::Other => {}
        }
    }
}

/// The order runs go in: each round lists the sets, every other round reversed (A B, B A, …).
pub(super) fn order(sets: &[String], rounds: usize) -> Vec<(usize, String)> {
    (0..rounds)
        .flat_map(|round| {
            let mut names = sets.to_vec();
            if round % 2 == 1 {
                names.reverse();
            }
            names.into_iter().map(move |name| (round, name))
        })
        .collect()
}

/// One set's results over its runs.
#[derive(Debug, PartialEq, Serialize)]
pub(super) struct Arm {
    pub name: String,
    pub runs: usize,
    pub passed: usize,
    pub pass_rate: f64,
    pub pass_all: bool,
    pub tokens: f64,
    pub cost: Option<f64>,
    pub secs: f64,
    pub skills: BTreeMap<String, usize>,
}

/// A paired difference, set minus baseline, with its bootstrap 95% interval.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub(super) struct Interval {
    pub diff: f64,
    pub low: f64,
    pub high: f64,
}

/// Whether the pass-rate interval clears zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Lift {
    Better,
    Worse,
    NoMeasuredLift,
}

/// One set against the baseline, paired by round.
#[derive(Debug, PartialEq, Serialize)]
pub(super) struct Pair {
    pub set: String,
    pub against: String,
    pub pass: Interval,
    pub tokens: Interval,
    pub lift: Lift,
}

/// The whole A/B result.
#[derive(Debug, PartialEq, Serialize)]
pub(super) struct Report {
    pub sets: Vec<Arm>,
    pub pairs: Vec<Pair>,
    pub runs: Vec<Run>,
}

impl Report {
    pub(super) fn build(sets: &[String], runs: Vec<Run>) -> Self {
        let arms = sets.iter().map(|name| arm(name, &runs)).collect();
        let base = sets
            .iter()
            .find(|name| *name == NONE)
            .or_else(|| sets.first())
            .cloned()
            .unwrap_or_default();
        let pairs = sets
            .iter()
            .filter(|name| **name != base)
            .map(|name| pair(name, &base, &runs))
            .collect();
        Self {
            sets: arms,
            pairs,
            runs,
        }
    }
}

fn arm(name: &str, runs: &[Run]) -> Arm {
    let mine: Vec<&Run> = runs.iter().filter(|run| run.set == name).collect();
    let passed = mine.iter().filter(|run| run.passed).count();
    let mut skills = BTreeMap::new();
    for skill in mine.iter().flat_map(|run| &run.skills) {
        *skills.entry(skill.clone()).or_insert(0) += 1;
    }
    let costs: Vec<f64> = mine.iter().filter_map(|run| run.cost).collect();
    Arm {
        name: name.to_owned(),
        runs: mine.len(),
        passed,
        pass_rate: ratio(passed, mine.len()),
        pass_all: !mine.is_empty() && passed == mine.len(),
        tokens: median(mine.iter().map(|run| run.tokens as f64).collect()),
        cost: (!costs.is_empty()).then(|| median(costs)),
        secs: median(mine.iter().map(|run| run.secs).collect()),
        skills,
    }
}

fn pair(set: &str, base: &str, runs: &[Run]) -> Pair {
    let find = |name: &str, round: usize| {
        runs.iter()
            .find(|run| run.set == name && run.round == round)
    };
    let pairs: Vec<(&Run, &Run)> = runs
        .iter()
        .filter(|run| run.set == set)
        .filter_map(|run| Some((run, find(base, run.round)?)))
        .collect();
    let passes: Vec<f64> = pairs
        .iter()
        .map(|(mine, theirs)| f64::from(u8::from(mine.passed)) - f64::from(u8::from(theirs.passed)))
        .collect();
    let tokens: Vec<f64> = pairs
        .iter()
        .map(|(mine, theirs)| mine.tokens as f64 - theirs.tokens as f64)
        .collect();
    let pass = interval(&passes, mean);
    let lift = if pass.low > 0.0 {
        Lift::Better
    } else if pass.high < 0.0 {
        Lift::Worse
    } else {
        Lift::NoMeasuredLift
    };
    Pair {
        set: set.to_owned(),
        against: base.to_owned(),
        pass,
        tokens: interval(&tokens, |values| median(values.to_vec())),
        lift,
    }
}

/// `statistic` of the paired differences, and the 2.5th and 97.5th percentiles over resamples.
fn interval(diffs: &[f64], statistic: fn(&[f64]) -> f64) -> Interval {
    if diffs.is_empty() {
        return Interval {
            diff: 0.0,
            low: 0.0,
            high: 0.0,
        };
    }
    let mut rng = fastrand::Rng::with_seed(SEED);
    let mut draws: Vec<f64> = (0..RESAMPLES)
        .map(|_| {
            let sample: Vec<f64> = (0..diffs.len())
                .map(|_| diffs[rng.usize(..diffs.len())])
                .collect();
            statistic(&sample)
        })
        .collect();
    draws.sort_by(f64::total_cmp);
    let at = |share: f64| draws[((draws.len() - 1) as f64 * share).round() as usize];
    Interval {
        diff: statistic(diffs),
        low: at(0.025),
        high: at(0.975),
    }
}

fn mean(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len().max(1) as f64
}

fn median(mut values: Vec<f64>) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f64::total_cmp);
    let mid = values.len() / 2;
    if values.len() % 2 == 0 {
        (values[mid - 1] + values[mid]) / 2.0
    } else {
        values[mid]
    }
}

fn ratio(part: usize, whole: usize) -> f64 {
    if whole == 0 {
        0.0
    } else {
        part as f64 / whole as f64
    }
}

#[cfg(test)]
#[path = "stats_tests.rs"]
mod tests;
