use std::collections::BTreeMap;
use std::sync::LazyLock;

use serde::Deserialize;

use super::Question;

#[derive(Deserialize)]
struct Set {
    name: String,
    questions: BTreeMap<String, Question>,
}

#[derive(Deserialize)]
struct Table {
    set: Vec<Set>,
}

static SETS: LazyLock<Vec<Set>> = LazyLock::new(|| {
    let Table { set } = summonr_core::embedded!("question set", "classify/sets.toml");
    set
});

static BY_NAME: LazyLock<BTreeMap<&'static str, &'static BTreeMap<String, Question>>> =
    LazyLock::new(|| {
        SETS.iter()
            .map(|set| (set.name.as_str(), &set.questions))
            .collect()
    });

pub static NAMES: LazyLock<Vec<&'static str>> =
    LazyLock::new(|| SETS.iter().map(|set| set.name.as_str()).collect());

pub fn by_name(name: &str) -> Option<BTreeMap<String, Question>> {
    BY_NAME.get(name).map(|&questions| questions.clone())
}
