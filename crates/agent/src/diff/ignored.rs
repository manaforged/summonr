enum Rule {
    Dir(&'static str),
    Suffix(&'static str),
    Under {
        dir: &'static str,
        keep: &'static [&'static str],
    },
}

const RULES: &[Rule] = &[
    Rule::Dir("__pycache__"),
    Rule::Suffix(".pyc"),
    Rule::Dir(".pytest_cache"),
    Rule::Dir(".mypy_cache"),
    Rule::Dir(".ruff_cache"),
    Rule::Under {
        dir: ".summonr",
        keep: &["check.toml", "policy.toml"],
    },
];

impl Rule {
    fn hides(&self, path: &str) -> bool {
        match self {
            Self::Dir(name) => path.split('/').rev().skip(1).any(|part| part == *name),
            Self::Suffix(suffix) => path.ends_with(suffix),
            Self::Under { dir, keep } => path
                .strip_prefix(*dir)
                .and_then(|rest| rest.strip_prefix('/'))
                .is_some_and(|rest| !keep.contains(&rest)),
        }
    }

    fn patterns(&self) -> Vec<String> {
        match self {
            Self::Dir(name) => vec![format!("{name}/")],
            Self::Suffix(suffix) => vec![format!("*{suffix}")],
            Self::Under { dir, keep } => std::iter::once(format!("/{dir}/*"))
                .chain(keep.iter().map(|name| format!("!/{dir}/{name}")))
                .collect(),
        }
    }
}

#[must_use]
pub(crate) fn hidden(path: &str) -> bool {
    RULES.iter().any(|rule| rule.hides(path))
}

pub(crate) fn patterns() -> Vec<String> {
    RULES.iter().flat_map(Rule::patterns).collect()
}
