use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Descendant {
    pub pid: u32,
    pub command: String,
}

pub(crate) fn descendants(root: u32) -> Vec<Descendant> {
    let mut children: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    let mut commands: BTreeMap<u32, String> = BTreeMap::new();
    for (pid, parent, command) in crate::process::table() {
        if pid != parent {
            children.entry(parent).or_default().push(pid);
        }
        commands.insert(pid, command);
    }
    let mut found = Vec::new();
    let mut pending: Vec<u32> = children.get(&root).cloned().unwrap_or_default();
    pending.reverse();
    while let Some(pid) = pending.pop() {
        if let Some(command) = commands.remove(&pid) {
            found.push(Descendant { pid, command });
        }
        if let Some(next) = children.get(&pid) {
            pending.extend(next.iter().rev());
        }
    }
    found
}

pub(crate) fn child_lines(root: u32, workspace: &std::path::Path) -> Vec<String> {
    let limits = crate::limits::limits();
    let found = descendants(root);
    let mut lines: Vec<String> = found
        .iter()
        .take(limits.job_child_rows)
        .map(|child| {
            let line = format!(
                "{} {}",
                child.pid,
                summonr_core::text::squash(&child.command)
            );
            crate::display::ellipsize(
                &crate::secret::shield::scrub_found(&line, workspace),
                limits.job_child_width,
            )
        })
        .collect();
    if found.len() > lines.len() {
        lines.push(format!("… {} more", found.len() - lines.len()));
    }
    lines
}

pub(crate) fn block(heading: &str, lines: &[String]) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let mut text = format!("{heading}:\n");
    for line in lines {
        text.push_str("  ");
        text.push_str(line);
        text.push('\n');
    }
    text
}
