use std::collections::BTreeSet;

use super::super::installed;

pub(crate) fn unknown_off(off: &[String]) -> Option<String> {
    if off.is_empty() {
        return None;
    }
    let installed = installed();
    let known: BTreeSet<&str> = installed
        .iter()
        .map(|plugin| plugin.name.as_str())
        .collect();
    let unknown: Vec<&str> = off
        .iter()
        .filter(|name| !known.contains(name.as_str()))
        .map(String::as_str)
        .collect();
    (!unknown.is_empty()).then(|| {
        format!(
            "[plugins] off names plugins that are not installed: {}; `summonr plugin remove NAME` removes the name",
            unknown.join(", ")
        )
    })
}

pub(crate) fn warn_unknown_off(off: &[String]) {
    if let Some(note) = unknown_off(off) {
        crate::display::note(&note);
    }
}
