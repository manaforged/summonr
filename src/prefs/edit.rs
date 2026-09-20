use std::path::Path;

pub(crate) fn banned_edit(
    _dest: &Path,
    _before: &str,
    _after: &str,
    _allowed: &[&str],
) -> Option<(&'static str, String)> {
    None
}

#[cfg(test)]
#[path = "edit_tests.rs"]
mod tests;
