use std::time::Instant;

use super::*;
use crate::tui::sigil_view::panes::ledger::LedgerRow;

fn model() -> Model {
    Model::new(&crate::tui::sigil_view::profile::info())
}

fn part(rank: Rank, text: &str, open: Option<StatusView>) -> Part {
    Part {
        rank,
        text: text.to_string(),
        tone: Token::Gray11,
        open,
    }
}

fn busy(parts: Vec<Part>) -> Status {
    Status {
        icon: Icon::Spinner,
        spinner: crate::prefs::Spinner::default(),
        clock: Some((Instant::now(), true)),
        rate: true,
        parts,
    }
}

fn full() -> Status {
    busy(vec![
        part(Rank::Verb, "Thinking…", None),
        part(
            Rank::Task,
            "2/5 Fix the auth refresh race in token.rs",
            Some(StatusView::Tasks),
        ),
        part(Rank::Agents, "3/11 agents", Some(StatusView::Agents)),
        part(Rank::Alert, "5 failed", None),
        part(Rank::Queued, "2 queued", None),
    ])
}

fn ranks(status: &Status) -> Vec<Rank> {
    status.parts.iter().map(|part| part.rank).collect()
}

fn ledger(lines: &[&str]) -> Vec<LedgerRow> {
    lines
        .iter()
        .filter_map(|line| LedgerRow::parse(line))
        .collect()
}

#[test]
fn a_wide_row_keeps_every_part() {
    let status = fit(full(), 160);
    assert_eq!(status.parts.len(), 5);
    assert!(status.rate);
    assert_eq!(status.clock.map(|(_, hint)| hint), Some(true));
}

#[test]
fn narrowing_drops_the_hint_then_the_rate_before_any_part() {
    let wide = fit(full(), 160);
    let used = wide.parts.iter().map(cost).sum::<usize>();
    let no_hint = fit(full(), used + 2 + 7 + RATE_COLS + 1 - 2);
    assert_eq!(no_hint.clock.map(|(_, hint)| hint), Some(false));
    assert!(no_hint.rate);
    assert_eq!(no_hint.parts.len(), 5);
    let no_rate = fit(full(), used + 2 + 7 - 2);
    assert!(!no_rate.rate);
    assert_eq!(no_rate.parts.len(), 5);
}

#[test]
fn the_task_shrinks_before_lower_parts_go() {
    let status = fit(full(), 70);
    let task = status
        .parts
        .iter()
        .find(|part| part.rank == Rank::Task)
        .expect("task kept");
    assert!(task.text.ends_with('…'), "{}", task.text);
    assert!(super::super::super::cell_width(&task.text) >= TASK_MIN);
}

#[test]
fn parts_leave_in_rank_order_and_the_verb_always_stays() {
    let mut seen = Vec::new();
    for width in (0..=160).rev() {
        let status = fit(full(), width);
        assert_eq!(status.parts.first().map(|part| part.rank), Some(Rank::Verb));
        let kept = ranks(&status);
        if seen.last() != Some(&kept) {
            seen.push(kept);
        }
    }
    let order: Vec<usize> = seen.iter().map(Vec::len).collect();
    assert!(order.windows(2).all(|pair| pair[0] >= pair[1]), "{order:?}");
    assert_eq!(seen.last(), Some(&vec![Rank::Verb]));
    let dropped: Vec<Rank> = seen
        .windows(2)
        .filter_map(|pair| pair[0].iter().find(|rank| !pair[1].contains(rank)).copied())
        .collect();
    assert_eq!(
        dropped,
        vec![Rank::Queued, Rank::Task, Rank::Agents, Rank::Alert]
    );
}

#[test]
fn every_fitted_row_fits_its_width() {
    for width in 24..=160 {
        let status = fit(full(), width);
        let fixed = 2
            + status
                .clock
                .map_or(0, |(_, hint)| if hint { CLOCK_COLS } else { 7 })
            + if status.rate { RATE_COLS + 1 } else { 0 };
        let used = (fixed + status.parts.iter().map(cost).sum::<usize>()).saturating_sub(2);
        assert!(used <= width || status.parts.len() == 1, "{width}: {used}");
    }
}

#[test]
fn the_verb_follows_the_live_state() {
    let mut model = model();
    assert_eq!(verb(&model, false, 0), None);
    assert_eq!(
        verb(&model, false, 2).as_deref(),
        Some("Waiting on agents…")
    );
    model.busy_since = Some((Activity::Turn, Instant::now()));
    model.doing = Some(Doing::Thinking);
    assert_eq!(verb(&model, false, 0).as_deref(), Some("Thinking…"));
    model.doing = Some(Doing::Tool {
        name: "run_command".to_string(),
        arg: "git diff".to_string(),
    });
    assert_eq!(verb(&model, false, 0).as_deref(), Some("Working…"));
    model.doing = None;
    assert_eq!(verb(&model, false, 0).as_deref(), Some("Finishing…"));
    model.entries.push(Entry::Assistant {
        text: "Half".to_string(),
        done: false,
    });
    model.speed.start(Instant::now());
    model.speed.delta(4, Instant::now());
    assert_eq!(verb(&model, false, 0).as_deref(), Some("Writing…"));
    model.speed.finish(1, Instant::now());
    assert_eq!(verb(&model, false, 0).as_deref(), Some("Finishing…"));
    assert_eq!(verb(&model, true, 0).as_deref(), Some("Waiting for you"));
    model.busy_since = Some((Activity::Compact, Instant::now()));
    assert_eq!(verb(&model, false, 0).as_deref(), Some("Compacting…"));
}

#[test]
fn a_reasoning_headline_replaces_thinking() {
    let mut model = model();
    model.busy_since = Some((Activity::Turn, Instant::now()));
    model.doing = Some(Doing::Thinking);
    model.entries.push(Entry::Thinking(
        "**Reading the config**\nsome notes\n**Planning the fix**\nmore".to_string(),
    ));
    assert_eq!(verb(&model, false, 0).as_deref(), Some("Planning the fix…"));
    model
        .entries
        .push(Entry::Thinking("no headline here".to_string()));
    assert_eq!(verb(&model, false, 0).as_deref(), Some("Thinking…"));
}

#[test]
fn the_task_names_the_active_step_and_strips_a_child_tag() {
    let mut model = model();
    assert_eq!(task(&model), None);
    model.ledger = ledger(&["[x] Read the code", "[/] Fix the race", "[ ] Ship it"]);
    assert_eq!(task(&model).as_deref(), Some("2/3 Fix the race"));
    model.ledger = ledger(&["[x] Read the code", "[x] Fix the race"]);
    assert_eq!(task(&model).as_deref(), Some("2/2 tasks"));
    model.ledger = ledger(&[
        "  [/] 00001111 · Check child result",
        "  [ ] 00001111 · Report",
    ]);
    assert_eq!(task(&model).as_deref(), Some("1/2 Check child result"));
    model.ledger = ledger(&[&format!("[/] {}", "very long step ".repeat(10))]);
    let text = task(&model).expect("task");
    assert!(
        super::super::super::cell_width(&text) <= TASK_COLS,
        "{text}"
    );
}
