use super::super::{Entry, Model};
use super::*;
use crate::tui::Activity;
use crate::tui::sigil_view::profile::info;

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
    let (_, marks) = slot_marks(&model, len - 2, entry::Slot::Lead(len - 2..len));
    let live: Vec<bool> = marks.iter().map(|mark| mark.2).collect();
    assert_eq!(live, [false, true]);
    assert_ne!(model.entries.rev(len - 3), model.entries.rev(len - 2));
    let revs: Vec<u64> = marks.iter().map(|mark| mark.3).collect();
    assert_eq!(
        revs,
        [model.entries.rev(len - 2), model.entries.rev(len - 1)]
    );
}
