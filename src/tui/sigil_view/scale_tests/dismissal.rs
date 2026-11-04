use super::children::{IDS, act, spawn};
use super::{State, busy_info, frame_with};
use crate::tools::Event;

#[derive(Clone, Copy)]
enum Action {
    ClearFirst,
    ClearSecond,
    Progress,
    Complete,
    Respawn,
}

const ACTIONS: [Action; 5] = [
    Action::ClearFirst,
    Action::ClearSecond,
    Action::Progress,
    Action::Complete,
    Action::Respawn,
];

#[test]
fn clearing_cards_preserves_lifecycle_under_every_four_event_order() {
    for sequence in 0..ACTIONS.len().pow(4) {
        check_sequence(sequence);
    }
}

fn check_sequence(mut sequence: usize) {
    frame_with(busy_info(), 100, 30, State::Busy, |app| {
        spawn(app, IDS[0], "First worker", true);
        spawn(app, IDS[1], "Second worker", true);
        let mut hidden = [false; 2];
        let mut done = false;
        for _ in 0..4 {
            let action = ACTIONS[sequence % ACTIONS.len()];
            sequence /= ACTIONS.len();
            match action {
                Action::ClearFirst => {
                    act(app, IDS[0], Event::Dismissed);
                    hidden[0] = true;
                }
                Action::ClearSecond => {
                    act(app, IDS[1], Event::Dismissed);
                    hidden[1] = true;
                }
                Action::Progress => act(
                    app,
                    IDS[0],
                    Event::Note {
                        text: "progress".into(),
                    },
                ),
                Action::Complete => {
                    act(
                        app,
                        IDS[0],
                        Event::Assistant {
                            text: "finished".into(),
                        },
                    );
                    done = true;
                }
                Action::Respawn => spawn(app, IDS[0], "First worker", true),
            }
            let model = app.model.get();
            assert_eq!(model.agents.len(), 2);
            assert_eq!(model.agents[0].done, done);
            assert!(!model.agents[1].done);
            assert!(model.busy_since.is_some());
            for (index, expected) in hidden.into_iter().enumerate() {
                assert_eq!(
                    model.agents_hidden.contains(&crate::child::tag(IDS[index])),
                    expected
                );
            }
        }
    });
}

#[test]
fn cleared_cards_stay_off_the_rendered_roster_after_late_progress() {
    let shown = frame_with(busy_info(), 140, 40, State::Busy, |app| {
        spawn(app, IDS[0], "Dismissed worker", true);
        act(app, IDS[0], Event::Dismissed);
        act(
            app,
            IDS[0],
            Event::Note {
                text: "late progress".into(),
            },
        );
        spawn(app, IDS[1], "Surviving worker", true);
        app.toggle_agents();
    })
    .join("\n");
    assert!(!shown.contains("Dismissed worker"), "{shown}");
    assert!(shown.contains("Surviving worker"), "{shown}");
}
