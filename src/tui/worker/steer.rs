use std::collections::VecDeque;
use std::sync::PoisonError;

use super::Cmd;
use crate::provider::UserInput;

pub(super) fn steerable(input: &UserInput) -> bool {
    let line = input.text.trim_start();
    input.images.is_empty()
        && super::command::shell::shell_prefix(line).is_none()
        && !line.starts_with('/')
}

pub(super) fn steer(
    steering: &crate::runtime::Steering,
    inputs: impl IntoIterator<Item = UserInput>,
    workspace: &std::path::Path,
) {
    let notes = crate::hunks::drain(workspace);
    let mut queue = steering.lock().unwrap_or_else(PoisonError::into_inner);
    queue.extend(inputs.into_iter().map(|mut input| {
        input.source.get_or_insert_with(|| crate::provider::Source {
            text: input.text.clone(),
            images: input.images.len(),
        });
        input
    }));
    queue.extend(notes.map(UserInput::from));
}

pub(super) fn take_inputs(
    deferred: &mut VecDeque<Cmd>,
    followups: &mut VecDeque<UserInput>,
    pick: impl Fn(&UserInput) -> bool,
) -> Vec<UserInput> {
    let mut taken = Vec::new();
    deferred.retain(|command| match command {
        Cmd::Submit(input) if pick(input) => {
            taken.push(input.clone());
            false
        }
        _other => true,
    });
    followups.retain(|input| {
        let take = pick(input);
        if take {
            taken.push(input.clone());
        }
        !take
    });
    taken
}
