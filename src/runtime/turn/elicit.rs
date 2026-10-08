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

    pub(super) async fn answer(&self, shared: &Steering) -> Option<UserInput> {
        tokio::time::timeout(crate::limits::limits().grant_wait(), shared.next())
            .await
            .ok()
    }
}

#[derive(Default)]
pub(crate) struct Steering {
    queue: Mutex<VecDeque<UserInput>>,
    wake: tokio::sync::Notify,
}

pub(crate) struct SteerGuard<'a> {
    queue: std::sync::MutexGuard<'a, VecDeque<UserInput>>,
    wake: &'a tokio::sync::Notify,
}

impl From<VecDeque<UserInput>> for Steering {
    fn from(queue: VecDeque<UserInput>) -> Self {
        Self {
            queue: Mutex::new(queue),
            wake: tokio::sync::Notify::new(),
        }
    }
}

impl Steering {
    pub(crate) fn lock(&self) -> std::sync::LockResult<SteerGuard<'_>> {
        let wake = &self.wake;
        match self.queue.lock() {
            Ok(queue) => Ok(SteerGuard { queue, wake }),
            Err(poisoned) => Err(std::sync::PoisonError::new(SteerGuard {
                queue: poisoned.into_inner(),
                wake,
            })),
        }
    }

    async fn next(&self) -> UserInput {
        loop {
            let notified = self.wake.notified();
            let popped = self
                .queue
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

impl std::ops::Deref for SteerGuard<'_> {
    type Target = VecDeque<UserInput>;

    fn deref(&self) -> &Self::Target {
        &self.queue
    }
}

impl std::ops::DerefMut for SteerGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.queue
    }
}

impl Drop for SteerGuard<'_> {
    fn drop(&mut self) {
        if !self.queue.is_empty() {
            self.wake.notify_one();
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
