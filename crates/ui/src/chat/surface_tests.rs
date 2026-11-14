use super::super::{Entry, Model};
use super::*;
use crate::profile::info;
use summonr_agent::hub::Activity;

fn receipts(count: usize) -> Model {
    let mut model = Model::new(&info());
    model.clear();
    for at in 0..count {
        model.entries.push(Entry::Receipt(format!("receipt {at}")));
    }
    model
}

#[test]
fn only_the_last_entry_of_a_live_group_is_marked_live() {
    let mut model = receipts(3);
    model.set_busy(Some(Activity::Turn));
    let len = model.entries.len();
    let (_, marks) = slot_revisions(&model, len - 2, entry::Slot::Lead(len - 2..len));
    let live: Vec<bool> = marks.iter().map(|mark| mark.1).collect();
    assert_eq!(live, [false, true]);
    assert_ne!(model.entries.rev(len - 3), model.entries.rev(len - 2));
    let revs: Vec<entry::Revision> = marks.iter().map(|mark| mark.0).collect();
    assert_eq!(
        revs,
        [
            entry::Revision::Entry(model.entries.rev(len - 2)),
            entry::Revision::Entry(model.entries.rev(len - 1))
        ]
    );
}

#[test]
fn terminal_revision_marks_only_the_last_entry_live_and_stacks_subagents() {
    let mut model = receipts(1);
    model.set_busy(Some(Activity::Turn));
    model.start_tool("a".to_owned(), "task".to_owned(), r#"{"background":true}"#.to_owned());
    model.start_tool("b".to_owned(), "task".to_owned(), r#"{"background":true}"#.to_owned());
    let len = model.entries.len();
    let first = terminal_revision(&model, len - 2);
    let second = terminal_revision(&model, len - 1);
    assert!(!first.1);
    assert!(first.2);
    assert!(second.1);
    assert!(!second.2);
}
