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
        let message = form.message.clone();
        let opened = Some(form.clone());
        let (dialog, input) = self
            .dialog(&shared, &message, events, emit, |dialog| Event::Elicit {
                form: opened,
                dialog,
            })
            .await?;
        let answer = match input {
            Some(input) => match serde_json::from_str::<FormAnswer>(input.text.trim()) {
                Ok(answer) => checked(&form, answer),
                Err(_) => {
                    self.steer.push_front(input);
                    FormAnswer::cancelled()
                }
            },
            None => FormAnswer::cancelled(),
        };
        self.push_event(events, emit, Event::Elicit { form: None, dialog })?;
        Ok(answer)
    }

    pub(super) async fn dialog(
        &mut self,
        shared: &Steering,
        status: &str,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
        open: impl FnOnce(u64) -> Event,
    ) -> Result<(u64, Option<UserInput>)> {
        let dialog = shared.open_dialog();
        let _blocked = self.herdr.blocked(status);
        self.push_event(events, emit, open(dialog))?;
        let input = tokio::time::timeout(crate::limits::limits().grant_wait(), shared.next())
            .await
            .ok();
        Ok((dialog, input))
    }
}

#[derive(Default)]
pub(crate) struct Steering {
    queue: Mutex<VecDeque<UserInput>>,
    answers: Mutex<VecDeque<UserInput>>,
    dialog: std::sync::atomic::AtomicU64,
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
            if let Event::Approval {
                detail: Some(_),
                dialog,
                ..
            }
            | Event::Elicit {
                form: Some(_),
                dialog,
            } = event
                && let Some(answer) = answers.pop_front()
            {
                steering.answer(*dialog, answer);
            }
        }
    }

    pub(crate) fn open_dialog(&self) -> u64 {
        let mut answers = self
            .answers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        answers.clear();
        self.dialog
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            .wrapping_add(1)
    }

    pub(crate) fn answer(&self, dialog: u64, input: UserInput) {
        let mut answers = self
            .answers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if dialog != self.dialog.load(std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        answers.push_back(input);
        drop(answers);
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
