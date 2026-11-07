use std::ops::Range;

use super::super::Entry;
use summonr_agent::tools::Builtin;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Slot {
    Single,
    Lead(Range<usize>),
    Member,
}

impl Slot {
    pub(crate) fn span(&self, index: usize) -> Range<usize> {
        match self {
            Self::Lead(range) => range.clone(),
            Self::Single => index..index + 1,
            Self::Member => index..index,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Run {
    Steps,
    Subagents,
}

fn run(entry: &Entry) -> Option<Run> {
    let name = match entry {
        Entry::Tool { name, .. } => name,
        Entry::User(_)
        | Entry::Thinking(_)
        | Entry::Receipt(_)
        | Entry::Assistant { .. }
        | Entry::Verify { .. }
        | Entry::Mail { .. }
        | Entry::Aborted(_)
        | Entry::System(_)
        | Entry::Injected(_)
        | Entry::Error(_)
        | Entry::Chart(_)
        | Entry::Image { .. } => return None,
    };
    Some(if Builtin::is(name, Builtin::Task) {
        Run::Subagents
    } else {
        Run::Steps
    })
}

pub(crate) fn subagents(entry: &Entry) -> bool {
    run(entry) == Some(Run::Subagents)
}

pub(crate) fn slots(entries: &[Entry]) -> Vec<Slot> {
    let mut found = Vec::with_capacity(entries.len());
    while found.len() < entries.len() {
        let index = found.len();
        match slot(entries, index) {
            Slot::Lead(range) => {
                let end = range.end;
                found.push(Slot::Lead(range));
                found.resize(end, Slot::Member);
            }
            other => found.push(other),
        }
    }
    found
}

fn joins(entry: &Entry, kind: Run) -> bool {
    run(entry) == Some(kind) || (kind == Run::Steps && matches!(entry, Entry::Thinking(_)))
}

pub(crate) fn slot(entries: &[Entry], index: usize) -> Slot {
    let Some(entry) = entries.get(index) else {
        return Slot::Single;
    };
    let Some(kind) = run(entry).or_else(|| joins(entry, Run::Steps).then_some(Run::Steps)) else {
        return Slot::Single;
    };
    let before = entries[..index]
        .iter()
        .rev()
        .take_while(|entry| joins(entry, kind))
        .count();
    let after = entries[index..]
        .iter()
        .take_while(|entry| joins(entry, kind))
        .count();
    let block = index - before..index + after;
    if !entries[block.clone()].iter().any(|entry| run(entry) == Some(kind)) {
        return Slot::Single;
    }
    if before > 0 {
        return Slot::Member;
    }
    Slot::Lead(block)
}

#[cfg(test)]
mod tests {
    use super::{Slot, slot};
    use crate::ToolState;
    use crate::model::{Call, Entry};

    fn tool(name: &str) -> Entry {
        Entry::Tool {
            name: name.to_owned(),
            calls: vec![Call::new(
                String::new(),
                "src/lib.rs".to_owned(),
                ToolState::Ok,
                String::new(),
            )],
        }
    }

    #[test]
    fn consecutive_tools_between_messages_fold_into_one_steps_group() {
        let entries = vec![
            Entry::User("fix it".to_owned()),
            Entry::Thinking("look first".to_owned()),
            tool("read"),
            Entry::Tool {
                name: "read".to_owned(),
                calls: vec![Call::new(
                    String::new(),
                    "a.rs".to_owned(),
                    ToolState::Ok,
                    String::new(),
                )],
            },
            tool("edit"),
            Entry::Assistant {
                text: "done".to_owned(),
                done: true,
            },
            tool("run_command"),
            tool("task"),
            tool("edit"),
        ];
        let slots: Vec<Slot> = (0..entries.len())
            .map(|index| slot(&entries, index))
            .collect();
        assert_eq!(
            slots,
            vec![
                Slot::Single,
                Slot::Lead(1..5),
                Slot::Member,
                Slot::Member,
                Slot::Member,
                Slot::Single,
                Slot::Lead(6..7),
                Slot::Lead(7..8),
                Slot::Lead(8..9),
            ]
        );
    }
}
