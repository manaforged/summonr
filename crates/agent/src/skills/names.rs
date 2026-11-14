use std::collections::HashMap;

/// How many entries carry each name, counted once so each lookup is constant time.
pub(crate) struct Names(HashMap<String, usize>);

impl Names {
    /// Counts each name in `names`.
    pub(crate) fn count<S: Into<String>>(names: impl IntoIterator<Item = S>) -> Self {
        let mut counts = HashMap::new();
        for name in names {
            *counts.entry(name.into()).or_insert(0) += 1;
        }
        Self(counts)
    }

    /// Whether exactly one entry carries `name`, so the bare name can stand for it.
    pub(crate) fn once(&self, name: &str) -> bool {
        self.0.get(name) == Some(&1)
    }

    /// Whether `/name` can stand for its one entry: exactly one entry carries it and no built-in command takes it.
    pub(crate) fn free(&self, name: &str) -> bool {
        self.once(name) && crate::parse_command(&format!("/{name}")).is_none()
    }
}
