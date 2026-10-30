use std::ops::Range;

use super::super::Entry;
use crate::tools::Builtin;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::tui::sigil_view) enum Slot {
    Single,
    Lead(Range<usize>),
    Member,
}

impl Slot {
    pub(in crate::tui::sigil_view) fn span(&self, index: usize) -> Range<usize> {
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

pub(in crate::tui::sigil_view) fn subagents(entry: &Entry) -> bool {
    run(entry) == Some(Run::Subagents)
}

pub(in crate::tui::sigil_view) fn slots(entries: &[Entry]) -> Vec<Slot> {
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

pub(in crate::tui::sigil_view) fn slot(entries: &[Entry], index: usize) -> Slot {
    let Some(kind) = entries.get(index).and_then(run) else {
        return Slot::Single;
    };
    if index > 0 && run(&entries[index - 1]) == Some(kind) {
        return Slot::Member;
    }
    let end = entries[index..]
        .iter()
        .position(|entry| run(entry) != Some(kind))
        .map_or(entries.len(), |offset| index + offset);
    Slot::Lead(index..end)
}

#[cfg(test)]
mod tests {
    use super::{Slot, slot};
    use crate::tui::ToolState;
    use crate::tui::sigil_view::model::{Call, Entry};

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
                Slot::Single,
                Slot::Lead(2..5),
                Slot::Member,
                Slot::Member,
                Slot::Single,
                Slot::Lead(6..7),
                Slot::Lead(7..8),
                Slot::Lead(8..9),
            ]
        );
    }

    #[cfg(feature = "gui")]
    #[test]
    fn edit_counts_skip_the_file_headers() {
        let patch = "--- a/x.rs\n+++ b/x.rs\n@@ -1,2 +1,2 @@\n-old\n+new\n+more\n same";
        let mut call = Call::new(
            String::new(),
            "x.rs".to_owned(),
            ToolState::Ok,
            String::new(),
        );
        call.patch = Some(patch.to_owned());
        assert_eq!(super::super::verb::patch_counts(&call), Some((2, 1)));
    }
}
