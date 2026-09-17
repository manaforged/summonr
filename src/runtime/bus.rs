use super::*;

impl<P: Provider> Loop<P> {
    pub(super) fn join_bus(&mut self) {
        self.bus = crate::bus::Bus::join(&self.session_id(), &self.model, &self.workspace);
    }

    pub(super) fn tool_definitions(&self) -> Vec<crate::provider::ToolDefinition> {
        let mut definitions = self.tools.definitions();
        if self.bus.is_some() {
            definitions.extend(crate::bus::definitions());
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
            self.schema = Some(self.tool_definitions());
        }
        self.schema.as_ref().map_or(&[], Vec::as_slice)
    }

    pub(super) fn bus_execute(&mut self, call: &ToolCall) -> Result<String> {
        let name = call.function.name.as_str();
        if self.bus.is_none() {
            return Err(Error::Tool(format!(
                "`{name}` needs the bus (set MAGE_BUS to enable it)"
            )));
        }
        let args = bus_args(call, name)?;
        let op = if name == "bus" {
            args.get("action")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| Error::Tool("bus requires action".to_string()))?
        } else {
            name
        };
        let text = bus_text(&args);
        let read_only = self.tools.is_read_only();
        if op == "inbox" {
            return Ok(inbox(self.take_all_bus_mail()?));
        }
        let Some(bus) = self.bus.as_mut() else {
            unreachable!();
        };
        match op {
            "agents" => agents(bus),
            "send" => send(bus, &args, text, read_only),
            "board" | "get" | "set" | "list" => board(bus, &self.workspace, &args, text, read_only),
            other => Err(Error::Tool(format!("unknown tool `{other}`"))),
        }
    }

    pub(crate) fn bus_heartbeat(&mut self, status: &str) {
        if let Some(bus) = &mut self.bus {
            bus.heartbeat(&self.model.clone(), &self.workspace.clone(), status);
        }
    }

    pub(crate) fn bus_poll(&mut self) -> Result<Vec<crate::bus::Mail>> {
        let mail = match &mut self.bus {
            Some(bus) => bus.poll()?,
            None => Vec::new(),
        };
        self.mail.extend(mail.iter().cloned());
        Ok(mail)
    }

    pub(super) fn take_bus_mail(&mut self, limit: usize) -> Result<(Vec<crate::bus::Mail>, usize)> {
        if let Some(bus) = &mut self.bus {
            self.mail.extend(bus.poll()?);
        }
        let count = self.mail.len().min(limit);
        let mail = self.mail.drain(..count).collect();
        Ok((mail, self.mail.len()))
    }

    fn take_all_bus_mail(&mut self) -> Result<Vec<crate::bus::Mail>> {
        if let Some(bus) = &mut self.bus {
            self.mail.extend(bus.poll()?);
        }
        Ok(self.mail.drain(..).collect())
    }

    pub(crate) fn bus_agents(&self) -> Vec<crate::bus::Agent> {
        match &self.bus {
            Some(bus) => bus.agents(),
            None => Vec::new(),
        }
    }
}

fn bus_args(call: &ToolCall, name: &str) -> Result<serde_json::Value> {
    if call.function.arguments.trim().is_empty() {
        return Ok(serde_json::json!({}));
    }
    serde_json::from_str(&call.function.arguments)
        .map_err(|error| Error::Tool(format!("{name} arguments are invalid: {error}")))
}

fn bus_text(args: &serde_json::Value) -> &str {
    args.get("text")
        .or_else(|| args.get("value"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
}

fn agents(bus: &crate::bus::Bus) -> Result<String> {
    Ok(bus.listing())
}

fn inbox(mail: Vec<crate::bus::Mail>) -> String {
    if mail.is_empty() {
        return "no new mail".to_string();
    }
    mail.iter()
        .map(crate::bus::envelope)
        .collect::<Vec<_>>()
        .join("\n")
}

fn send(
    bus: &crate::bus::Bus,
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
    let sent = bus.send(to, text)?;
    Ok(if to == "all" {
        format!("sent to {sent} session(s)")
    } else {
        format!("sent to {to}")
    })
}

fn board(
    bus: &mut crate::bus::Bus,
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
        "set" => board_set(bus, workspace, args, text, read_only),
        "get" => board_get(bus, workspace, args),
        _ => board_list(bus, workspace),
    }
}

fn board_set(
    bus: &mut crate::bus::Bus,
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
    bus.post(workspace, key, text)?;
    Ok(format!("posted {key}"))
}

fn board_get(bus: &crate::bus::Bus, workspace: &Path, args: &serde_json::Value) -> Result<String> {
    let key = args
        .get("key")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Error::Tool("board get requires key".to_string()))?;
    let hit = bus.board(workspace)?.into_iter().find(|(k, _, _)| k == key);
    Ok(match hit {
        Some((_, value, from)) => format!("{key} = {value} (from {from})"),
        None => format!("no post for {key}"),
    })
}

fn board_list(bus: &crate::bus::Bus, workspace: &Path) -> Result<String> {
    let board = bus.board(workspace)?;
    if board.is_empty() {
        return Ok("the board is empty".to_string());
    }
    Ok(board
        .iter()
        .map(|(key, value, from)| format!("{key} = {value} (from {from})"))
        .collect::<Vec<_>>()
        .join("\n"))
}
