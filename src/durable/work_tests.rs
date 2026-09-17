use super::{Policy, StepState, Work};

#[test]
fn a_single_turn_todo_list_survives_work_progress() {
    let mut work = Work::new("ship", Policy::Single);
    work.apply_todos(&[
        ("decode the image".to_string(), StepState::Active),
        ("write the overlay".to_string(), StepState::Pending),
    ]);
    let progress = work.progress();
    assert!(
        progress
            .ledger
            .iter()
            .any(|line| line.contains("[/]") && line.contains("decode the image")),
        "{:?}",
        progress.ledger
    );
    assert!(
        progress
            .ledger
            .iter()
            .any(|line| line.contains("[ ]") && line.contains("write the overlay")),
        "{:?}",
        progress.ledger
    );
}
