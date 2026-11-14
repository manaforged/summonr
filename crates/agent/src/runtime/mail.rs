use super::*;

impl<P: Provider> Loop<P> {
    pub(super) fn join_mail(&mut self) {
        self.roster();
    }

    pub(super) fn roster(&mut self) {
        let id = self.session_id();
        let name = self.name();
        if let Some(mailbox) = self
            .mailbox
            .as_mut()
            .filter(|mailbox| mailbox.owned_by(&id))
        {
            mailbox.refresh(name.as_deref(), &self.model, &self.workspace);
            return;
        }
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
        let steering = self.steer_shared.clone();
        async move {
            match steering {
                Some(shared) => shared.changed().await,
                None => std::future::pending().await,
            }
            Ok(())
        }
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

    /// The workspace whose board this session reads.
    pub(crate) fn board_root(&self) -> &Path {
        self.mailbox
            .as_ref()
            .map_or(&self.workspace, crate::mail::Mailbox::board_root)
    }

    /// Puts this session on the board at `root`.
    pub(crate) fn share_board(&mut self, root: &Path) {
        let (model, workspace) = (self.model.clone(), self.workspace.clone());
        if let Some(mailbox) = &mut self.mailbox {
            mailbox.share_board(root, &model, &workspace);
        }
    }

    /// Whether any of `mail` should start a turn in this session.
    pub(crate) fn mail_wakes(&self, mail: &[crate::mail::Mail]) -> bool {
        self.mailbox
            .as_ref()
            .is_some_and(|mailbox| mail.iter().any(|item| mailbox.wakes(item)))
    }

    /// Gives this session the mail tools so it can lead a swarm; fails when its tool list is already fixed without them.
    pub(crate) fn enable_swarm(&mut self) -> Result<()> {
        if self.mailbox.is_none() {
            return Err(Error::Config(
                "/swarm needs the mail roster, and SUMMONR_MAIL is off".to_string(),
            ));
        }
        if !self.task_available() {
            return Err(Error::Config(
                "/swarm needs the task module and a model that can run child agents".to_string(),
            ));
        }
        if crate::mail::tools(self.mail_module) {
            return Ok(());
        }
        if self.catalog_frozen() {
            return Err(Error::Config(
                "this session's tools are fixed and have no board; start a session with /new and run /swarm first".to_string(),
            ));
        }
        self.mail_module = true;
        self.schema = None;
        Ok(())
    }

    /// The board's claims and newest posts, as `board list` shows them.
    pub(crate) fn board_listing(&self) -> Result<String> {
        let mailbox = self
            .mailbox
            .as_ref()
            .ok_or_else(|| Error::Tool("this session is not on the mail roster".to_string()))?;
        board_list(mailbox)
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
    args: &serde_json::Value,
    text: &str,
    read_only: bool,
) -> Result<String> {
    let action = args
        .get("action")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("list");
    match action {
        "post" => board_post(mailbox, text, read_only),
        "set" => board_set(mailbox, args, text, read_only),
        "get" => board_get(mailbox, args),
        _ => board_list(mailbox),
    }
}

fn board_post(mailbox: &crate::mail::Mailbox, text: &str, read_only: bool) -> Result<String> {
    if read_only {
        return Err(Error::Tool(
            "board post is disabled in a read-only session".to_string(),
        ));
    }
    if text.is_empty() {
        return Err(Error::Tool("board post requires value".to_string()));
    }
    let (sent, id) = mailbox.say(text)?;
    Ok(format!("posted message {id} to {sent} agent(s) on this board"))
}

fn board_set(
    mailbox: &mut crate::mail::Mailbox,
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
    mailbox.post(key, text)?;
    Ok(format!("posted {key}"))
}

fn board_get(
    mailbox: &crate::mail::Mailbox,
    args: &serde_json::Value,
) -> Result<String> {
    let key = args
        .get("key")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Error::Tool("board get requires key".to_string()))?;
    Ok(crate::mail::board_entry(&mailbox.board()?, key))
}

fn board_list(mailbox: &crate::mail::Mailbox) -> Result<String> {
    Ok(crate::mail::board_text(&mailbox.board()?, &mailbox.posts()?))
}

pub(crate) struct Advertised {
    pub(crate) tools: Vec<crate::provider::ToolDefinition>,
    pub(crate) json: crate::provider::ToolsJson,
    pub(crate) patch: bool,
}
