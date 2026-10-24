use super::*;

impl<P: Provider> Loop<P> {
    pub(super) fn inject(&mut self, text: String, messages: &mut Vec<Message>) -> Result<()> {
        let input = UserInput::from(text);
        self.session.steer(&input).map_err(super::session_err)?;
        messages.push(Message::user(input));
        Ok(())
    }
}

const STEER_NOTE: &str = "Harness: the user sent this while you were working. Answer it in text in your next reply, then change course if it asks.";

pub(super) fn drain_steer(
    queue: &mut std::collections::VecDeque<UserInput>,
    session: &mut SessionDriver,
    messages: &mut Vec<Message>,
) -> Result<()> {
    while let Some(input) = queue.pop_front() {
        let steered = with_note(&input);
        if let Err(error) = session.steer(&steered) {
            queue.push_front(input);
            return Err(super::session_err(error));
        }
        messages.push(Message::user(steered));
    }
    Ok(())
}

fn with_note(input: &UserInput) -> UserInput {
    let mut steered = input.clone();
    if input.source.is_some() {
        steered.text = crate::session::decorate(STEER_NOTE, &input.text);
    }
    steered
}

#[cfg(test)]
#[path = "steer_tests.rs"]
mod tests;
