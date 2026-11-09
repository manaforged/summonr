use std::collections::BTreeMap;
use std::sync::LazyLock;

use serde::Deserialize;
use summonr_core::catalog::{Catalog, Row};

use super::Question;

#[derive(Deserialize)]
struct Set {
    name: String,
    questions: BTreeMap<String, Question>,
}

impl Row for Set {
    fn name(&self) -> &str {
        &self.name
    }
}

#[derive(Deserialize)]
struct Table {
    set: Vec<Set>,
}

static SETS: LazyLock<Catalog<Set>> = LazyLock::new(|| {
    let Table { set } = summonr_core::embedded!("question set", "classify/sets.toml");
    Catalog::from_rows("question set", set).unwrap_or_else(|error| panic!("{error}"))
});

pub static NAMES: LazyLock<Vec<&'static str>> =
    LazyLock::new(|| SETS.rows().iter().map(|set| set.name.as_str()).collect());

pub fn by_name(name: &str) -> Option<BTreeMap<String, Question>> {
    SETS.get(name).map(|set| set.questions.clone())
}
