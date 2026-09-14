pub(super) const SPLIT: usize = 80;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Pair {
    pub(super) left: Option<String>,
    pub(super) right: Option<String>,
    pub(super) kind: Kind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Meta,
    Hunk,
    Context,
    Change,
}

pub(super) fn pairs(text: &str) -> Vec<Pair> {
    let mut rows = Vec::new();
    let mut removed: Vec<String> = Vec::new();
    let mut added: Vec<String> = Vec::new();
    let flush = |rows: &mut Vec<Pair>, removed: &mut Vec<String>, added: &mut Vec<String>| {
        let count = removed.len().max(added.len());
        for index in 0..count {
            rows.push(Pair {
                left: removed.get(index).cloned(),
                right: added.get(index).cloned(),
                kind: Kind::Change,
            });
        }
        removed.clear();
        added.clear();
    };
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix('-')
            && !line.starts_with("---")
        {
            removed.push(rest.to_string());
        } else if let Some(rest) = line.strip_prefix('+')
            && !line.starts_with("+++")
        {
            added.push(rest.to_string());
        } else {
            flush(&mut rows, &mut removed, &mut added);
            let kind = if line.starts_with("@@") {
                Kind::Hunk
            } else if let Some(rest) = line.strip_prefix(' ') {
                rows.push(Pair {
                    left: Some(rest.to_string()),
                    right: Some(rest.to_string()),
                    kind: Kind::Context,
                });
                continue;
            } else {
                Kind::Meta
            };
            rows.push(Pair {
                left: Some(line.to_string()),
                right: None,
                kind,
            });
        }
    }
    flush(&mut rows, &mut removed, &mut added);
    rows
}

#[cfg(test)]
#[path = "patch_tests.rs"]
mod tests;
