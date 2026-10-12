use super::*;

impl<P: Provider> Loop<P> {
    pub(super) fn inject(&mut self, text: String, messages: &mut Vec<Message>) -> Result<()> {
        let input = UserInput::from(text);
        self.session.steer(&input).map_err(super::session_err)?;
        messages.push(Message::user(input));
        Ok(())
    }
}

#[cfg(test)]
#[path = "steer_tests.rs"]
mod tests;
