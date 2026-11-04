use super::*;

impl<P: Provider> Loop<P> {
    pub(super) fn join_mail(&mut self) {
        self.roster();
    }

    pub(super) fn roster(&mut self) {
        drop(self.mailbox.take());
        self.mailbox = crate::mail::Mailbox::join(
            &self.session_id(),
            self.name().as_deref(),
            &self.model,
            &self.workspace,
        );
    }

    pub(super) fn tool_definitions(&self) -> Vec<crate::provider::ToolDefinition> {
        let mut definitions = self.tools.definitions();
        if self.mailbox.is_some() && crate::mail::tools(self.mail_module) {
            definitions.extend(crate::mail::definitions());
        }
        if let Some(hub) = &self.mcp {
            definitions.extend(hub.definitions(self.tools.policy()));
        }
        if self.task_available() {
            definitions.push(super::child::definition());
        }
        if self.ask_available() {
            definitions.push(super::turn::ask_definition());
        }
        crate::tools::shaped(definitions, crate::provider::traits(&self.model).patch)
    }

    pub(super) fn advertised_tools(&mut self) -> Arc<Advertised> {
        let patch = crate::provider::traits(&self.model).patch;
        if let Some(advertised) = self.schema.as_ref().filter(|cached| cached.patch == patch) {
            return Arc::clone(advertised);
        }
        let tools = self.tool_definitions();
        let json = crate::provider::ToolsJson::encode(&tools);
        drop(self.session.note_schema(json.len() as u64));
        let advertised = Arc::new(Advertised { tools, json, patch });
        self.schema = Some(Arc::clone(&advertised));
        advertised
    }

    pub(super) fn mail_execute(&mut self, call: &ToolCall) -> Result<String> {
        let name = call.function.name.as_str();
        let Some(tool) = crate::mail::Tool::of(name) else {
            return Err(Error::Tool(format!("unknown tool `{name}`")));
        };
        if self.mailbox.is_none() || !crate::mail::tools(self.mail_module) {
            return Err(Error::Tool(format!(
                "`{name}` needs the mail module (/modules mail on)"
            )));
        }
        let arguments = call.function.arguments.trim();
        let args = crate::tools::parse_args(
            name,
            if arguments.is_empty() {
                "{}"
            } else {
                arguments
            },
        )?;
        let text = mail_text(&args);
        let read_only = self.tools.is_read_only();
        let missing = || Error::Tool("the mailbox is not connected".to_string());
        match tool {
            crate::mail::Tool::Inbox => Ok(crate::mail::inbox_text(&self.take_all_mail()?)),
            crate::mail::Tool::Agents => agents(self.mailbox.as_mut().ok_or_else(missing)?),
            crate::mail::Tool::Send => send(
                self.mailbox.as_mut().ok_or_else(missing)?,
                &args,
                text,
                read_only,
            ),
            crate::mail::Tool::Board => board(
                self.mailbox.as_mut().ok_or_else(missing)?,
                &self.workspace,
                &args,
                text,
                read_only,
            ),
        }
    }

    pub(crate) fn mail_heartbeat(&mut self, status: crate::mail::Presence) {
        if let Some(mailbox) = &mut self.mailbox {
            mailbox.heartbeat(&self.model.clone(), &self.workspace.clone(), status);
        }
    }

    pub(crate) fn mail_note_tool(&mut self, tool: &str, arguments: &str) {
        let parent = self.session.parent().map(|id| id.to_string());
        if let Some(mailbox) = &mut self.mailbox {
            mailbox.note_tool(
                &self.model.clone(),
                &self.workspace.clone(),
                (tool, arguments),
                parent.as_deref(),
            );
        }
    }

    pub(crate) fn mail_poll(&mut self) -> Result<Vec<crate::mail::Mail>> {
        let mail = match &mut self.mailbox {
            Some(mailbox) => mailbox.poll()?,
            None => Vec::new(),
        };
        self.inbox.extend(mail.iter().cloned());
        Ok(mail)
    }

    pub(in crate::runtime) fn input_interrupts(&self) -> bool {
        self.session.depth() > 0
    }

    pub(super) fn steered(&self, lead: &str) -> String {
        if !self.input_interrupts() {
            return lead.to_string();
        }
        let notes = match self.tools.stop_jobs() {
            Ok(notes) => notes,
            Err(error) => vec![format!("could not stop background jobs: {error}")],
        };
        if notes.is_empty() {
            return lead.to_string();
        }
        let body = notes.join("\n");
        if lead.is_empty() {
            body
        } else {
            format!("{lead}\n{body}")
        }
    }

    pub(super) fn input_wait(&self) -> impl std::future::Future<Output = Result<()>> + use<P> {
        self.watch_input(true)
    }

    pub(super) fn input_cut(&self) -> impl std::future::Future<Output = Result<()>> + use<P> {
        let input = self.input_wait();
        let grace = if self.input_interrupts() {
            0
        } else {
            crate::limits::limits().tool_yield_secs
        };
        async move {
            input.await?;
            tokio::time::sleep(crate::limits::seconds(grace)).await;
            Ok(())
        }
    }

    pub(super) fn reply_input_wait(
        &self,
    ) -> impl std::future::Future<Output = Result<()>> + use<P> {
        self.watch_input(self.input_interrupts())
    }

    fn watch_input(
        &self,
        with_mail: bool,
    ) -> impl std::future::Future<Output = Result<()>> + use<P> {
        let steering = self.steer_shared.clone();
        let mail = self
            .mailbox
            .as_ref()
            .filter(|_mailbox| with_mail)
            .map(crate::mail::Mailbox::changed);
        let queued = with_mail && !self.inbox.is_empty();
        async move {
            if queued {
                return Ok(());
            }
            tokio::select! {
                () = async {
                    match steering {
                        Some(shared) => shared.changed().await,
                        None => std::future::pending().await,
                    }
                } => Ok(()),
                result = async {
                    match mail {
                        Some(changed) => changed.await,
                        None => std::future::pending().await,
                    }
                } => result,
            }
        }
    }

    pub(super) fn take_peer_mail(
        &mut self,
        limit: usize,
    ) -> Result<(Vec<crate::mail::Mail>, usize)> {
        if let Some(mailbox) = &mut self.mailbox {
            self.inbox.extend(mailbox.poll()?);
        }
        let count = self.inbox.len().min(limit);
        let mail = self.inbox.drain(..count).collect();
        Ok((mail, self.inbox.len()))
    }

    fn take_all_mail(&mut self) -> Result<Vec<crate::mail::Mail>> {
        if let Some(mailbox) = &mut self.mailbox {
            self.inbox.extend(mailbox.poll()?);
        }
        Ok(self.inbox.drain(..).collect())
    }

    pub(crate) fn peers(&self) -> Vec<crate::mail::Agent> {
        match &self.mailbox {
            Some(mailbox) => mailbox.agents(),
            None => Vec::new(),
        }
    }
}

fn mail_text(args: &serde_json::Value) -> &str {
    args.get("text")
        .or_else(|| args.get("value"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
}

fn agents(mailbox: &crate::mail::Mailbox) -> Result<String> {
    Ok(mailbox.listing())
}

fn send(
    mailbox: &crate::mail::Mailbox,
    args: &serde_json::Value,
    text: &str,
    read_only: bool,
) -> Result<String> {
    if read_only {
        return Err(Error::Tool(
            "send is disabled in a read-only session".to_string(),
        ));
    }
    let to = args
        .get("to")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Error::Tool("send requires to".to_string()))?;
    if text.is_empty() {
        return Err(Error::Tool("send requires text".to_string()));
    }
    let reply_to = args.get("reply_to").and_then(serde_json::Value::as_str);
    let (sent, id) = mailbox.send(to, text, reply_to)?;
    Ok(if to == "all" {
        format!("sent message {id} to {sent} session(s)")
    } else {
        format!("sent message {id} to {to}")
    })
}

fn board(
    mailbox: &mut crate::mail::Mailbox,
    workspace: &Path,
    args: &serde_json::Value,
    text: &str,
    read_only: bool,
) -> Result<String> {
    let action = args
        .get("action")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("list");
    match action {
        "set" => board_set(mailbox, workspace, args, text, read_only),
        "get" => board_get(mailbox, workspace, args),
        _ => board_list(mailbox, workspace),
    }
}

fn board_set(
    mailbox: &mut crate::mail::Mailbox,
    workspace: &Path,
    args: &serde_json::Value,
    text: &str,
    read_only: bool,
) -> Result<String> {
    if read_only {
        return Err(Error::Tool(
            "board set is disabled in a read-only session".to_string(),
        ));
    }
    let key = args
        .get("key")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Error::Tool("board set requires key".to_string()))?;
    mailbox.post(workspace, key, text)?;
    Ok(format!("posted {key}"))
}

fn board_get(
    mailbox: &crate::mail::Mailbox,
    workspace: &Path,
    args: &serde_json::Value,
) -> Result<String> {
    let key = args
        .get("key")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Error::Tool("board get requires key".to_string()))?;
    Ok(crate::mail::board_entry(&mailbox.board(workspace)?, key))
}

fn board_list(mailbox: &crate::mail::Mailbox, workspace: &Path) -> Result<String> {
    Ok(crate::mail::board_text(&mailbox.board(workspace)?))
}

pub(crate) struct Advertised {
    pub(crate) tools: Vec<crate::provider::ToolDefinition>,
    pub(crate) json: crate::provider::ToolsJson,
    pub(crate) patch: bool,
}
