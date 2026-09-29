use super::*;
use crate::WorkKind;

impl<P: Provider> Loop<P> {
    pub(super) fn join_mail(&mut self) {
        self.mailbox = None;
        if self.depth == 0 {
            self.mailbox = crate::mail::Mailbox::join(
                &self.session_id(),
                self.name().as_deref(),
                &self.model,
                &self.workspace,
            );
        }
    }

    pub(super) fn tool_definitions(&self) -> Vec<crate::provider::ToolDefinition> {
        let work = self
            .work_progress()
            .map_or(WorkKind::Single, |progress| progress.kind);
        let mut definitions = self.tools.definitions(work);
        if self.mailbox.is_some() && crate::mail::tools(self.mail_module) {
            definitions.extend(crate::mail::definitions());
        }
        if let Some(hub) = &self.mcp {
            definitions.extend(hub.definitions(self.tools.is_read_only()));
        }
        if self.task_available() {
            definitions.push(super::child::definition());
        }
        definitions
    }

    pub(super) fn advertised_tools(&mut self) -> &[crate::provider::ToolDefinition] {
        if self.schema.is_none() {
            let definitions = self.tool_definitions();
            let bytes = serde_json::to_vec(&definitions).map_or(0, |encoded| encoded.len() as u64);
            drop(self.session.note_schema(bytes));
            self.schema = Some(definitions);
        }
        self.schema.as_ref().map_or(&[], Vec::as_slice)
    }

    pub(super) fn mail_execute(&mut self, call: &ToolCall) -> Result<String> {
        let name = call.function.name.as_str();
        if self.mailbox.is_none() || !crate::mail::tools(self.mail_module) {
            return Err(Error::Tool(format!(
                "`{name}` needs the mail module (/modules mail on)"
            )));
        }
        let args = mail_args(call, name)?;
        let text = mail_text(&args);
        let read_only = self.tools.is_read_only();
        if name == "inbox" {
            return Ok(crate::mail::inbox_text(&self.take_all_mail()?));
        }
        let Some(mailbox) = self.mailbox.as_mut() else {
            return Err(Error::Tool("the mailbox is not connected".to_string()));
        };
        match name {
            "agents" => agents(mailbox),
            "send" => send(mailbox, &args, text, read_only),
            "board" => board(mailbox, &self.workspace, &args, text, read_only),
            other => Err(Error::Tool(format!("unknown tool `{other}`"))),
        }
    }

    pub(crate) fn mail_heartbeat(&mut self, status: &str) {
        if let Some(mailbox) = &mut self.mailbox {
            mailbox.heartbeat(&self.model.clone(), &self.workspace.clone(), status);
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

fn mail_args(call: &ToolCall, name: &str) -> Result<serde_json::Value> {
    if call.function.arguments.trim().is_empty() {
        return Ok(serde_json::json!({}));
    }
    serde_json::from_str(&call.function.arguments)
        .map_err(|error| Error::Tool(format!("{name} arguments are invalid: {error}")))
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
        return Err(Error::Tool("send is disabled in plan mode".to_string()));
    }
    let to = args
        .get("to")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Error::Tool("send requires to".to_string()))?;
    if text.is_empty() {
        return Err(Error::Tool("send requires text".to_string()));
    }
    let sent = mailbox.send(to, text)?;
    Ok(if to == "all" {
        format!("sent to {sent} session(s)")
    } else {
        format!("sent to {to}")
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
            "board set is disabled in plan mode".to_string(),
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
