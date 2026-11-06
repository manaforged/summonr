use super::*;

impl<P: Provider> Loop<P> {
    pub(super) async fn task_control(
        &mut self,
        args: &Args,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<Execution> {
        let id = args
            .child
            .as_deref()
            .ok_or_else(|| Error::Tool("task control requires child".to_string()))?;
        if args.clear && !args.kill {
            return self.task_clear(id, events, emit);
        }
        let mut execution = self.task_poll(id, args.kill, events, emit).await?;
        if args.clear {
            execution.output = self.task_clear(id, events, emit)?.output;
        }
        Ok(execution)
    }

    fn task_clear(
        &mut self,
        id: &str,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<Execution> {
        self.drain(emit);
        let child = id
            .parse::<SessionId>()
            .ok()
            .and_then(|id| self.session.child(id));
        if !self.spawned.contains_key(id) && child.is_none() {
            return Err(Error::Tool(format!("unknown child `{id}`")));
        }
        self.push_event(
            events,
            emit,
            Event::Child {
                id: id.to_string(),
                event: Box::new(Event::Dismissed),
            },
        )?;
        Ok(Execution {
            output: ToolOutput::ok(format!("cleared child {id}; session and history retained")),
            usage: Usage::default(),
            millis: 0,
        })
    }
}
