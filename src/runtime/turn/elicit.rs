use super::*;
use crate::mcp::{Form, FormAction, FormAnswer};

impl<P: Provider> Loop<P> {
    pub(super) async fn elicit(
        &mut self,
        form: Form,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<FormAnswer> {
        let Some(shared) = self.steer_shared.clone() else {
            return Ok(FormAnswer::declined());
        };
        let _blocked = self.herdr.blocked(&form.message);
        self.push_event(
            events,
            emit,
            Event::Elicit {
                form: Some(form.clone()),
            },
        )?;
        let answer = match self.answer(&shared).await {
            Some(input) => match serde_json::from_str::<FormAnswer>(input.text.trim()) {
                Ok(answer) => checked(&form, answer),
                Err(_) => {
                    self.steer.push_front(input);
                    FormAnswer::cancelled()
                }
            },
            None => FormAnswer::cancelled(),
        };
        self.push_event(events, emit, Event::Elicit { form: None })?;
        Ok(answer)
    }

    pub(super) async fn answer(
        &self,
        shared: &Arc<Mutex<VecDeque<UserInput>>>,
    ) -> Option<UserInput> {
        let deadline = Instant::now() + crate::limits::GRANT_WAIT;
        while Instant::now() < deadline {
            if let Some(input) = shared.lock().ok().and_then(|mut queue| queue.pop_front()) {
                return Some(input);
            }
            tokio::time::sleep(crate::limits::GRANT_POLL).await;
        }
        None
    }
}

fn checked(form: &Form, answer: FormAnswer) -> FormAnswer {
    if answer.action != FormAction::Accept {
        return FormAnswer {
            content: None,
            ..answer
        };
    }
    let content = answer.content.unwrap_or_default();
    match form.check(&content) {
        Ok(()) => FormAnswer {
            action: FormAction::Accept,
            content: Some(content),
        },
        Err(_) => FormAnswer::cancelled(),
    }
}

#[cfg(test)]
#[path = "elicit_tests.rs"]
mod tests;
