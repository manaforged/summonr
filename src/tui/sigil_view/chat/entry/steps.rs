use std::ops::Range;

use super::super::Entry;
use crate::tools::Builtin;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::tui::sigil_view) enum Slot {
    Single,
    Lead(Range<usize>),
    Member,
}

fn step(entry: &Entry) -> bool {
    match entry {
        Entry::Tool { name, .. } => !Builtin::is(name, Builtin::Task),
        Entry::ToolGroup { name, .. } => !Builtin::is(name, Builtin::Task),
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
        | Entry::Image { .. } => false,
    }
}

pub(in crate::tui::sigil_view) fn slot(entries: &[Entry], index: usize) -> Slot {
    if !entries.get(index).is_some_and(step) {
        return Slot::Single;
    }
    if index > 0 && step(&entries[index - 1]) {
        return Slot::Member;
    }
    let end = entries[index..]
        .iter()
        .position(|entry| !step(entry))
        .map_or(entries.len(), |offset| index + offset);
    Slot::Lead(index..end)
}

pub(in crate::tui::sigil_view) fn counts(patch: &str) -> (usize, usize) {
    patch
        .lines()
        .filter(|line| !line.starts_with("+++ ") && !line.starts_with("--- "))
        .fold((0, 0), |(adds, dels), line| {
            if line.starts_with('+') {
                (adds + 1, dels)
            } else if line.starts_with('-') {
                (adds, dels + 1)
            } else {
                (adds, dels)
            }
        })
}

#[cfg(test)]
mod tests {
    use super::{Slot, counts, slot};
    use crate::tui::ToolState;
    use crate::tui::sigil_view::model::{Call, Entry};

    fn tool(name: &str) -> Entry {
        Entry::Tool {
            id: String::new(),
            name: name.to_owned(),
            arg: "src/lib.rs".to_owned(),
            state: ToolState::Ok,
            output: String::new(),
            expanded: false,
            patch: None,
        }
    }

    #[test]
    fn consecutive_tools_between_messages_fold_into_one_steps_group() {
        let entries = vec![
            Entry::User("fix it".to_owned()),
            Entry::Thinking("look first".to_owned()),
            tool("read"),
            Entry::ToolGroup {
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
                Slot::Single,
                Slot::Lead(8..9),
            ]
        );
    }

    #[test]
    fn edit_counts_skip_the_file_headers() {
        let patch = "--- a/x.rs\n+++ b/x.rs\n@@ -1,2 +1,2 @@\n-old\n+new\n+more\n same";
        assert_eq!(counts(patch), (2, 1));
    }
}
