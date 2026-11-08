use serde::Deserialize;
use serde_json::Value;

use super::Window;

const FIVE_HOURS: u64 = 300;
const WEEK: u64 = 10_080;

/// A usage reply format.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Shape {
    /// ChatGPT `backend-api/wham/usage`.
    Wham,
    /// Codex app-server `account/rateLimits/read`, at `/result/rateLimits`.
    Codex,
    /// z.ai and BigModel `monitor/usage/quota/limit`.
    Zai,
    /// Antigravity `agy -p /usage --output-format json`.
    Antigravity,
    /// A JSON list of windows, as `summonr endpoint usage --json` prints them.
    Windows,
}

impl Shape {
    pub(super) fn parse(self, value: &Value) -> Result<Vec<Window>, String> {
        let windows = match self {
            Self::Wham => wham(value),
            Self::Codex => codex(value),
            Self::Zai => zai(value),
            Self::Antigravity => antigravity(value),
            Self::Windows => serde_json::from_value(value.clone()).map_err(|error| error.to_string())?,
        };
        if windows.is_empty() {
            return Err(format!("no usage windows in the {self:?} reply"));
        }
        Ok(windows)
    }
}

/// A short name for a window length.
pub(super) fn label(minutes: Option<u64>) -> String {
    match minutes {
        Some(FIVE_HOURS) => "5-hour".to_string(),
        Some(WEEK) => "week".to_string(),
        Some(minutes) if minutes % 1440 == 0 => format!("{} d", minutes / 1440),
        Some(minutes) if minutes % 60 == 0 => format!("{} h", minutes / 60),
        Some(minutes) => format!("{minutes} min"),
        None => "window".to_string(),
    }
}

fn wham(value: &Value) -> Vec<Window> {
    ["primary_window", "secondary_window"]
        .iter()
        .filter_map(|key| {
            let window = value["rate_limit"].get(key)?;
            Some(Window {
                group: None,
                minutes: window["limit_window_seconds"].as_u64().map(|seconds| seconds / 60),
                used: window["used_percent"].as_f64()?,
                resets: window["reset_at"].as_i64().and_then(|at| time(at, 1)),
            })
        })
        .collect()
}

fn codex(value: &Value) -> Vec<Window> {
    ["primary", "secondary"]
        .iter()
        .filter_map(|key| {
            let window = value.get(key).filter(|window| !window.is_null())?;
            Some(Window {
                group: None,
                minutes: window["windowDurationMins"].as_u64(),
                used: window["usedPercent"].as_f64()?,
                resets: window["resetsAt"].as_i64().and_then(|at| time(at, 1)),
            })
        })
        .collect()
}

/// Claude Code's `rate_limit_event` `unifiedWindows`, with utilization from 0 to 1.
pub(super) fn unified(value: &Value) -> Vec<Window> {
    [("five_hour", FIVE_HOURS), ("seven_day", WEEK)]
        .iter()
        .filter_map(|(key, minutes)| {
            let window = value.get(key)?;
            Some(Window {
                group: None,
                minutes: Some(*minutes),
                used: window["utilization"].as_f64()? * 100.0,
                resets: window["resetsAt"].as_i64().and_then(|at| time(at, 1)),
            })
        })
        .collect()
}

fn zai(value: &Value) -> Vec<Window> {
    let Some(limits) = value["data"]["limits"].as_array() else {
        return Vec::new();
    };
    limits
        .iter()
        .filter(|limit| matches!(limit["type"].as_str(), Some("TOKENS_LIMIT" | "CREDIT_LIMIT")))
        .filter_map(|limit| {
            let unit = match limit["unit"].as_u64()? {
                1 => 1440,
                3 => 60,
                5 => 1,
                6 => WEEK,
                _ => return None,
            };
            Some(Window {
                group: None,
                minutes: limit["number"].as_u64().map(|number| number * unit),
                used: limit["percentage"].as_f64()?,
                resets: limit["nextResetTime"].as_i64().and_then(|at| time(at, 1000)),
            })
        })
        .collect()
}

fn antigravity(value: &Value) -> Vec<Window> {
    let Some(groups) = value["command"]["data"]["groups"].as_array() else {
        return Vec::new();
    };
    groups
        .iter()
        .flat_map(|group| {
            let name = group["name"].as_str().map(str::to_string);
            group["buckets"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|bucket| !bucket["disabled"].as_bool().unwrap_or(false))
                .filter_map(move |bucket| {
                    Some(Window {
                        group: name.clone(),
                        minutes: match bucket["window"].as_str()? {
                            "5h" => Some(FIVE_HOURS),
                            "weekly" => Some(WEEK),
                            _ => None,
                        },
                        used: (1.0 - bucket["remaining_fraction"].as_f64()?) * 100.0,
                        resets: bucket["reset_time"].as_str().map(str::to_string),
                    })
                })
        })
        .collect()
}

fn time(at: i64, per_second: i64) -> Option<String> {
    jiff::Timestamp::from_second(at / per_second)
        .ok()
        .map(|time| time.to_string())
}
