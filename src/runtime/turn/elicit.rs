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
        shared.reset_answers();
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

    pub(super) async fn answer(&self, shared: &Steering) -> Option<UserInput> {
        tokio::time::timeout(crate::limits::limits().grant_wait(), shared.next())
            .await
            .ok()
    }
}

#[derive(Default)]
pub(crate) struct Steering {
    queue: Mutex<VecDeque<UserInput>>,
    answers: Mutex<VecDeque<UserInput>>,
    wake: tokio::sync::Notify,
}

impl Steering {
    pub(crate) fn lock(
        &self,
    ) -> std::sync::LockResult<std::sync::MutexGuard<'_, VecDeque<UserInput>>> {
        self.queue.lock()
    }

    #[cfg(test)]
    pub(crate) fn replying(self: &Arc<Self>, answers: &[&str]) -> impl FnMut(&Event) + use<> {
        let steering = Arc::clone(self);
        let mut answers: VecDeque<UserInput> = answers
            .iter()
            .map(|answer| UserInput::from(*answer))
            .collect();
        move |event| {
            if matches!(
                event,
                Event::Approval {
                    detail: Some(_),
                    ..
                } | Event::Elicit { form: Some(_) }
            ) && let Some(answer) = answers.pop_front()
            {
                steering.answer(answer);
            }
        }
    }

    pub(super) fn reset_answers(&self) {
        self.answers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }

    pub(crate) fn answer(&self, input: UserInput) {
        self.answers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push_back(input);
        self.wake.notify_one();
    }

    async fn next(&self) -> UserInput {
        loop {
            let notified = self.wake.notified();
            let popped = self
                .answers
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .pop_front();
            if let Some(input) = popped {
                return input;
            }
            notified.await;
        }
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
