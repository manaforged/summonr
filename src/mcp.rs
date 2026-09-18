use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use tokio::sync::Mutex;

use crate::error::{Error, Result};
use crate::provider::ToolDefinition;
use crate::rpc::{self, Client};
use crate::runtime::JOB_WAIT;
use crate::tools::{ToolOutput, tool};

mod config;
mod http;

pub(crate) use config::Entry;
use config::{Launch, Server};

const STATELESS: &str = "2026-07-28";

pub(crate) struct Hub {
    links: Vec<Link>,
    pub(crate) notes: Vec<String>,
}

struct Link {
    server: Server,
    workspace: PathBuf,
    conn: Mutex<Option<Conn>>,
    tools: Vec<Tool>,
}

struct Tool {
    alias: String,
    name: String,
    description: String,
    schema: Value,
    readonly: bool,
}

enum Transport {
    Stdio(Box<Client>),
    Http(http::Http),
}

struct Conn {
    transport: Transport,
    stateless: bool,
}

impl Hub {
    pub(crate) async fn connect(workspace: PathBuf) -> Self {
        let servers = config::servers(&workspace);
        let opened = futures::future::join_all(servers.into_iter().map(|server| {
            let workspace = workspace.clone();
            async move {
                let name = server.name.clone();
                match tokio::time::timeout(JOB_WAIT, Link::open(server, workspace)).await {
                    Ok(Ok(link)) => Ok(link),
                    Ok(Err(error)) => Err(format!("mcp {name}: {error}")),
                    Err(_elapsed) => {
                        Err(format!("mcp {name}: no answer in {}s", JOB_WAIT.as_secs()))
                    }
                }
            }
        }))
        .await;
        let mut hub = Self {
            links: Vec::new(),
            notes: Vec::new(),
        };
        for result in opened {
            match result {
                Ok(link) => hub.links.push(link),
                Err(note) => hub.notes.push(note),
            }
        }
        hub
    }

    pub(crate) fn definitions(&self, read_only: bool) -> Vec<ToolDefinition> {
        self.links
            .iter()
            .flat_map(|link| &link.tools)
            .filter(|tool| tool.readonly || !read_only)
            .map(|item| tool(&item.alias, &item.description, item.schema.clone()))
            .collect()
    }

    pub(crate) fn owns(&self, alias: &str) -> bool {
        self.find(alias).is_some()
    }

    pub(crate) async fn call(
        &self,
        alias: &str,
        arguments: &str,
        read_only: bool,
    ) -> Result<ToolOutput> {
        let (link, item) = self
            .find(alias)
            .ok_or_else(|| Error::Tool(format!("unknown tool `{alias}`")))?;
        if read_only && !item.readonly {
            return Err(Error::Denied(format!(
                "`{alias}` can change state and is unavailable in plan mode"
            )));
        }
        let arguments: Value = if arguments.trim().is_empty() {
            json!({})
        } else {
            serde_json::from_str(arguments)
                .map_err(|error| Error::Tool(format!("{alias} arguments are invalid: {error}")))?
        };
        let result = link
            .call(
                "tools/call",
                json!({"name": item.name, "arguments": arguments}),
            )
            .await?;
        Ok(output(&result, !item.readonly))
    }

    fn find(&self, alias: &str) -> Option<(&Link, &Tool)> {
        self.links.iter().find_map(|link| {
            link.tools
                .iter()
                .find(|item| item.alias == alias)
                .map(|item| (link, item))
        })
    }
}

impl Link {
    async fn open(server: Server, workspace: PathBuf) -> Result<Self> {
        let mut conn = Conn::open(&server, &workspace).await?;
        let mut tools = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let params = cursor.map_or_else(|| json!({}), |cursor| json!({"cursor": cursor}));
            let page = conn.call("tools/list", params).await?;
            tools.extend(
                page.get("tools")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|item| Tool::parse(&server.name, item)),
            );
            cursor = page
                .get("nextCursor")
                .and_then(Value::as_str)
                .map(str::to_string);
            if cursor.is_none() {
                break;
            }
        }
        Ok(Self {
            server,
            workspace,
            conn: Mutex::new(Some(conn)),
            tools,
        })
    }

    async fn call(&self, method: &str, params: Value) -> Result<Value> {
        let mut slot = self.conn.lock().await;
        let mut conn = match slot.take() {
            Some(conn) => conn,
            None => Conn::open(&self.server, &self.workspace).await?,
        };
        let result = conn.call(method, params).await;
        if result.is_ok() || conn.alive() {
            *slot = Some(conn);
        }
        result
    }
}

impl Conn {
    async fn open(server: &Server, workspace: &Path) -> Result<Self> {
        let transport = match &server.launch {
            Launch::Stdio { command, args, env } => {
                let args: Vec<&str> = args.iter().map(String::as_str).collect();
                Transport::Stdio(Box::new(Client::spawn(command, &args, env, workspace)?))
            }
            Launch::Http { url, headers, auth } => Transport::Http(http::Http::new(
                url,
                headers,
                auth.clone()
                    .map(|spec| crate::credentials::mcp_auth(spec, &format!("mcp-{}", server.name)))
                    .transpose()?,
            )?),
        };
        let mut conn = Self {
            transport,
            stateless: false,
        };
        let greeted = match &mut conn.transport {
            Transport::Stdio(client) => client.initialize("summonr").await.map(drop),
            Transport::Http(http) => http.initialize(rpc::hello("summonr")).await,
        };
        conn.stateless = greeted.is_err();
        Ok(conn)
    }

    fn alive(&mut self) -> bool {
        match &mut self.transport {
            Transport::Stdio(client) => client.alive(),
            Transport::Http(_) => true,
        }
    }

    async fn call(&mut self, method: &str, mut params: Value) -> Result<Value> {
        if self.stateless
            && let Some(object) = params.as_object_mut()
        {
            object.insert(
                "_meta".to_string(),
                json!({
                    "io.modelcontextprotocol/protocolVersion": STATELESS,
                    "io.modelcontextprotocol/clientInfo": {"name": "summonr", "version": env!("CARGO_PKG_VERSION")}
                }),
            );
        }
        match &mut self.transport {
            Transport::Stdio(client) => client.call(method, params, |_, _| {}).await,
            Transport::Http(http) => http.call(method, params).await,
        }
    }
}

impl Tool {
    fn parse(server: &str, item: &Value) -> Option<Self> {
        let name = item.get("name")?.as_str()?.to_string();
        let alias: String = format!("{server}__{name}")
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '_' | '-') {
                    c
                } else {
                    '_'
                }
            })
            .take(64)
            .collect();
        Some(Self {
            alias,
            description: item
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            schema: item
                .get("inputSchema")
                .cloned()
                .unwrap_or_else(|| json!({"type": "object"})),
            readonly: item
                .pointer("/annotations/readOnlyHint")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            name,
        })
    }
}

fn output(result: &Value, mutated: bool) -> ToolOutput {
    let mut parts: Vec<String> = result
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| match item.get("type").and_then(Value::as_str)? {
            "text" => item.get("text").and_then(Value::as_str).map(str::to_string),
            "resource" => item
                .pointer("/resource/text")
                .or_else(|| item.pointer("/resource/uri"))
                .and_then(Value::as_str)
                .map(str::to_string),
            "resource_link" => item.get("uri").and_then(Value::as_str).map(str::to_string),
            kind => Some(format!("[{kind} content omitted]")),
        })
        .collect();
    if parts.is_empty()
        && let Some(structured) = result.get("structuredContent")
    {
        parts.push(structured.to_string());
    }
    ToolOutput {
        text: parts.join("\n"),
        success: !result
            .get("isError")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        mutated,
        grant: None,
    }
}
