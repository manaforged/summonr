use super::*;

impl<P: Provider> Loop<P> {
    pub(super) fn inject(&mut self, text: String) -> Result<()> {
        self.session.steer(&UserInput::from(text))?;
        Ok(())
    }
}

const STEER_NOTE: &str = "Harness: the user sent this while you were working. Fold it into the current work and keep going; answer in one line only if it asks a question.";

pub(super) fn drain_steer(
    queue: &mut std::collections::VecDeque<UserInput>,
    session: &mut SessionDriver,
) -> Result<Vec<String>> {
    let mut delivered = Vec::new();
    while let Some(input) = queue.pop_front() {
        let steered = with_note(&input);
        if let Err(error) = session.steer(&steered) {
            queue.push_front(input);
            return Err(Error::from(error));
        }
        delivered.extend(input.source.map(|source| source.text));
    }
    Ok(delivered)
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
