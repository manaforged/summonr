use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Outcome {
    pub command: String,
    pub passed: bool,
    pub output: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Receipt {
    pub changes: Vec<String>,
    pub checks: Vec<Outcome>,
    #[serde(default)]
    pub snapshot: String,
    #[serde(default)]
    pub aborted: bool,
    #[serde(default)]
    pub verified: bool,
    pub text: String,
}
