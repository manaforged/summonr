pub(in crate::tui::sigil_view) fn matching(
    choices: Vec<crate::endpoint::AuthInfo>,
    filter: &str,
) -> Vec<crate::endpoint::AuthInfo> {
    let query = filter.to_lowercase();
    choices
        .into_iter()
        .filter(|choice| choice.matches(&query))
        .collect()
}

pub(super) fn best_match(rows: &[crate::endpoint::AuthInfo], filter: &str) -> usize {
    let query = filter.trim().to_lowercase();
    rows.iter()
        .enumerate()
        .min_by_key(|(index, choice)| {
            let name = choice.name.to_lowercase();
            (name != query, !name.starts_with(&query), *index)
        })
        .map_or(0, |(index, _)| index)
}
