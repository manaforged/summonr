use sigil::prelude::fuzzy_score;

#[must_use]
pub(crate) fn matching_commands(typed: &str) -> Vec<(&'static str, &'static str)> {
    if !typed.starts_with('/') || typed.contains('\n') || typed.contains('\r') {
        return Vec::new();
    }
    let query = typed.to_lowercase();
    let needle = query.trim_start_matches('/');
    let shown = || crate::command_specs().iter().filter(|spec| spec.shown());
    let mut prefix: Vec<_> = shown()
        .filter(|spec| spec.name.trim_start_matches('/').starts_with(needle))
        .collect();
    if !prefix.is_empty() {
        prefix.sort_by_key(|spec| spec.name);
        return prefix
            .into_iter()
            .map(|spec| (spec.name, spec.description))
            .collect();
    }
    if needle.contains(char::is_whitespace) {
        return Vec::new();
    }
    let mut matches: Vec<_> = shown()
        .filter_map(|spec| {
            let name = spec.name.trim_start_matches('/');
            rank(name, spec.description, needle).map(|score| (score, spec.name, spec))
        })
        .collect();
    matches.sort_unstable_by_key(|(score, name, _)| (*score, *name));
    if let Some(best) = matches.first().map(|row| row.0) {
        matches.retain(|(score, _, _)| *score == best);
    }
    matches
        .into_iter()
        .map(|(_, name, spec)| (name, spec.description))
        .collect()
}

fn rank(name: &str, description: &str, needle: &str) -> Option<usize> {
    let named = fuzzy_score(needle, name).map(|found| found.score);
    named
        .filter(|score| *score < 40)
        .or_else(|| description.to_lowercase().contains(needle).then_some(35))
        .or(named)
}
