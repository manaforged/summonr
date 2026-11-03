use super::Model;
use super::panes::ledger::{LedgerRow, Mark};

pub(in crate::tui::sigil_view) fn tally(ledger: &[LedgerRow]) -> (usize, usize) {
    ledger
        .iter()
        .filter(|row| row.mark != Mark::Plain)
        .fold((0, 0), |(done, total), row| {
            (done + usize::from(row.mark == Mark::Done), total + 1)
        })
}

pub(super) fn current(model: &Model) -> Option<&LedgerRow> {
    model
        .ledger
        .iter()
        .find(|row| row.mark == Mark::Active)
        .or_else(|| model.ledger.iter().find(|row| !row.mark.settled()))
}
