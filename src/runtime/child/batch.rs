use super::record::*;
use super::*;

impl<P: Provider> Loop<P> {
    pub(in crate::runtime) async fn task_execute(
        &mut self,
        call: &ToolCall,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<Execution> {
        self.tasks_execute(std::slice::from_ref(call), events, emit)
            .await
            .pop()
            .ok_or_else(|| Error::Config("task execution returned no result".to_string()))?
    }

    pub(in crate::runtime) async fn tasks_execute(
        &mut self,
        calls: &[ToolCall],
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Vec<Result<Execution>> {
        if calls.is_empty() {
            return Vec::new();
        }
        if self.depth != 0 {
            return rejected(calls.len(), "child sessions cannot delegate another task");
        }
        let mut args = Vec::with_capacity(calls.len());
        for call in calls {
            match task_args(call) {
                Ok(mut value) => {
                    value.call.clone_from(&call.id);
                    args.push(value);
                }
                Err(error) => return rejected(calls.len(), &error.to_string()),
            }
        }
        if args
            .iter()
            .all(|args| args.child.is_none() && !args.background)
        {
            return self.task_batch(args, events, emit).await;
        }
        let mut background = Vec::new();
        let mut results: Vec<Option<Result<Execution>>> =
            std::iter::repeat_with(|| None).take(args.len()).collect();
        for (index, args) in args.into_iter().enumerate() {
            if args.child.is_none() && args.background {
                background.push((index, args));
                continue;
            }
            let result = match args.child.clone() {
                Some(id) => self.task_poll(&id, args.kill, events, emit).await,
                None => self
                    .task_batch(vec![args], events, emit)
                    .await
                    .pop()
                    .unwrap_or_else(|| {
                        Err(Error::Config(
                            "task execution returned no result".to_string(),
                        ))
                    }),
            };
            results[index] = Some(result);
        }
        if !background.is_empty() {
            let budget = match self.task_budget(background.len()) {
                Ok(budget) => budget,
                Err(error) => {
                    let message = error.to_string();
                    for (index, _) in background {
                        results[index] = Some(Err(Error::Config(message.clone())));
                    }
                    return results
                        .into_iter()
                        .map(|result| {
                            result.unwrap_or_else(|| {
                                Err(Error::Config("task result was not recorded".to_string()))
                            })
                        })
                        .collect();
                }
            };
            for (index, args) in background {
                results[index] = Some(self.task_spawn(args, &budget));
            }
        }
        results
            .into_iter()
            .map(|result| {
                result.unwrap_or_else(|| {
                    Err(Error::Config("task result was not recorded".to_string()))
                })
            })
            .collect()
    }

    pub(super) async fn task_batch(
        &mut self,
        args: Vec<Args>,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Vec<Result<Execution>> {
        let count = args.len();
        let budget = match self.task_budget(count) {
            Ok(budget) => budget,
            Err(error) => return rejected(count, &error.to_string()),
        };
        let mut runs = Vec::with_capacity(count);
        for args in args {
            let access = args.access;
            runs.push((access, self.task_start(args, &budget, None)));
        }
        let sender = self.mailbox.0.clone();
        let mut runs: FuturesUnordered<_> = runs
            .into_iter()
            .enumerate()
            .map(|(index, (access, run))| {
                let sender = sender.clone();
                async move {
                    let done = match run {
                        Ok(run) => Ok(run.execute(sender).await),
                        Err(error) => Err(error),
                    };
                    (index, access, done)
                }
            })
            .collect();
        let mut finished = Vec::with_capacity(count);
        while finished.len() < count {
            tokio::select! {
                Some((index, access, done)) = runs.next() => {
                    let done = match done {
                        Ok(mut done) if access == Access::Research => {
                            self.task_record(&mut done).map(|()| done)
                        }
                        other => other,
                    };
                    finished.push((index, access, done));
                }
                Some(event) = self.mailbox.1.recv() => {
                    self.saw(&event);
                    emit(&event);
                    events.push(event);
                }
            }
        }
        finished.sort_by_key(|(index, _, _)| *index);
        let mut done: Vec<(Access, Result<Done>)> = finished
            .into_iter()
            .map(|(_, access, done)| (access, done))
            .collect();
        self.drain(events, emit);
        self.task_reconcile(&mut done);
        let mut results = Vec::with_capacity(done.len());
        for (_, done) in done {
            results.push(done.and_then(|done| self.task_settle(done)));
        }
        results
    }

    pub(super) fn task_record(&mut self, done: &mut Done) -> Result<()> {
        self.session
            .finish_child(done.record.clone())
            .map_err(super::session_err)?;
        self.session
            .charge(
                super::meter(&done.usage, 0),
                &format!("child {}", done.record.id),
                &done.record.model,
                true,
                UsageMeta::default(),
            )
            .map_err(super::session_err)?;
        let reported =
            u64::from(done.usage.prompt_tokens) + u64::from(done.usage.completion_tokens);
        if done.billed > reported {
            self.session
                .charge(
                    Meter {
                        input: done.billed - reported,
                        ..Meter::default()
                    },
                    &format!("child {} unconfirmed usage", done.record.id),
                    &done.record.model,
                    false,
                    UsageMeta::default(),
                )
                .map_err(super::session_err)?;
        }
        self.spent_tokens = self.session.spent_tokens();
        done.settled = true;
        Ok(())
    }
}
