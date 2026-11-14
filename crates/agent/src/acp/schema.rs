use serde_json::{Map, Value, json};

use super::ext::{self, KEY, METHODS, Method, NOTES, Note, PREFIX, STATES, VERSION};
use super::{PROTOCOL, codes, config, permission, update};
use crate::rpc::Ask;

const ACP: &str = "https://agentclientprotocol.com/protocol/schema";

const IN: &str = "client_to_agent";
const OUT: &str = "agent_to_client";

#[derive(Default)]
struct Surface {
    definitions: Map<String, Value>,
    methods: Map<String, Value>,
    notifications: Map<String, Value>,
}

impl Surface {
    fn call(&mut self, name: &str, params: Value, result: Value, direction: &str) {
        let params = define(&mut self.definitions, &format!("{name}.params"), params);
        let result = define(&mut self.definitions, &format!("{name}.result"), result);
        let entry = json!({"direction": direction, "params": params, "result": result});
        self.methods.insert(name.to_string(), entry);
    }

    fn notify(&mut self, name: &str, params: Value, direction: &str) {
        let params = define(&mut self.definitions, &format!("{name}.params"), params);
        let entry = json!({"direction": direction, "params": params});
        self.notifications.insert(name.to_string(), entry);
    }
}

/// The JSON Schema of the ACP surface `summonr acp` serves, with the summonr extension.
#[must_use]
pub fn schema() -> Value {
    let mut surface = Surface::default();
    for (ask, params, result) in standard() {
        surface.call(ask.name().unwrap_or_default(), params, result, IN);
    }
    surface.call(
        permission::METHOD,
        permission_params(),
        permission_result(),
        OUT,
    );
    for (method, name) in METHODS {
        let (params, result) = extension(method);
        surface.call(&format!("{PREFIX}{name}"), params, result, IN);
    }
    let cancel = Ask::SessionCancel.name().unwrap_or_default();
    surface.notify(cancel, session(json!({}), &[]), IN);
    surface.notify(update::METHOD, session_update(), OUT);
    for (kind, name) in NOTES {
        surface.notify(&format!("{PREFIX}{name}"), notice(kind), OUT);
    }
    let Surface {
        mut definitions,
        methods,
        notifications,
    } = surface;
    definitions.insert("ContentBlock".to_string(), content_block());
    definitions.insert("McpServerStdio".to_string(), mcp_server());
    definitions.insert("ConfigOption".to_string(), config_option());
    definitions.insert("SessionOpened".to_string(), opened());
    let errors: Vec<Value> = codes::ALL
        .iter()
        .map(|(code, meaning)| json!({"code": code, "meaning": meaning}))
        .collect();
    json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "title": "summonr ACP surface",
        "description": format!("The methods summonr serves over `summonr acp`, with the fields it reads and writes. Standard ACP types in full: {ACP}"),
        "protocolVersion": PROTOCOL,
        "version": VERSION,
        "extension": {"key": KEY, "prefix": PREFIX, "features": ext::features()},
        "methods": methods,
        "notifications": notifications,
        "errors": errors,
        "definitions": definitions,
    })
}

fn define(definitions: &mut Map<String, Value>, name: &str, schema: Value) -> Value {
    definitions.insert(name.to_string(), schema);
    reference(name)
}

fn reference(name: &str) -> Value {
    json!({"$ref": format!("#/definitions/{}", name.replace('~', "~0").replace('/', "~1"))})
}

fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type": "object", "properties": properties, "required": required})
}

fn session(mut properties: Value, required: &[&str]) -> Value {
    properties["sessionId"] = json!({"type": "string"});
    let mut all = vec!["sessionId"];
    all.extend_from_slice(required);
    object(properties, &all)
}

fn empty() -> Value {
    json!({"type": "object", "additionalProperties": false})
}

fn strings(values: &[&str]) -> Value {
    json!({"type": "string", "enum": values})
}

fn standard() -> Vec<(Ask, Value, Value)> {
    let servers = json!({"type": "array", "items": reference("McpServerStdio")});
    let opened = reference("SessionOpened");
    let prompt = json!({"type": "array", "items": reference("ContentBlock")});
    let options = json!({"type": "array", "items": reference("ConfigOption")});
    vec![
        (Ask::Initialize, initialize_params(), initialize_result()),
        (Ask::Authenticate, json!({"type": "object"}), empty()),
        (
            Ask::SessionNew,
            object(
                json!({"cwd": {"type": "string"}, "mcpServers": servers}),
                &["cwd"],
            ),
            opened.clone(),
        ),
        (
            Ask::SessionLoad,
            session(
                json!({"cwd": {"type": "string"}, "mcpServers": servers}),
                &["cwd"],
            ),
            opened,
        ),
        (
            Ask::SessionPrompt,
            session(json!({"prompt": prompt}), &["prompt"]),
            object(
                json!({"stopReason": strings(&config::STOPS)}),
                &["stopReason"],
            ),
        ),
        (
            Ask::SessionSetConfigOption,
            session(
                json!({"configId": strings(&[config::EFFORT]), "value": strings(&config::values())}),
                &["configId", "value"],
            ),
            object(json!({"configOptions": options}), &["configOptions"]),
        ),
    ]
}

fn initialize_params() -> Value {
    let opt_in = json!({"type": "object", "properties": {
        KEY: object(json!({"version": {"const": VERSION}}), &["version"]),
    }});
    object(
        json!({
            "protocolVersion": {"type": "integer"},
            "clientCapabilities": {"type": "object", "properties": {"_meta": opt_in.clone()}},
            "_meta": opt_in,
        }),
        &["protocolVersion"],
    )
}

fn initialize_result() -> Value {
    let echo = object(
        json!({KEY: object(
            json!({
                "version": {"const": VERSION},
                "features": {"type": "array", "items": strings(&ext::features())},
            }),
            &["version", "features"],
        )}),
        &[KEY],
    );
    let transports = json!({"http": {"const": false}, "sse": {"const": false}});
    object(
        json!({
            "protocolVersion": {"const": PROTOCOL},
            "agentCapabilities": object(
                json!({
                    "loadSession": {"const": true},
                    "mcpCapabilities": object(transports, &["http", "sse"]),
                    "_meta": echo,
                }),
                &["loadSession", "mcpCapabilities"],
            ),
            "agentInfo": object(
                json!({"name": {"type": "string"}, "version": {"type": "string"}}),
                &["name", "version"],
            ),
            "authMethods": {"type": "array", "maxItems": 0},
        }),
        &[
            "protocolVersion",
            "agentCapabilities",
            "agentInfo",
            "authMethods",
        ],
    )
}

fn permission_params() -> Value {
    let ids: Vec<&str> = permission::CHOICES.iter().map(|choice| choice.id).collect();
    let kinds: Vec<&str> = permission::CHOICES
        .iter()
        .map(|choice| choice.kind)
        .collect();
    let option = object(
        json!({"optionId": strings(&ids), "name": {"type": "string"}, "kind": strings(&kinds)}),
        &["optionId", "name", "kind"],
    );
    session(
        json!({
            "toolCall": object(
                json!({"toolCallId": {"type": "string"}, "title": {"type": "string"}}),
                &["toolCallId", "title"],
            ),
            "options": {"type": "array", "items": option},
        }),
        &["toolCall", "options"],
    )
}

fn permission_result() -> Value {
    let outcome = object(
        json!({"outcome": {"type": "string"}, "optionId": {"type": "string"}}),
        &["outcome"],
    );
    let mut result = object(json!({"outcome": outcome}), &["outcome"]);
    result["description"] = json!(
        "Only outcome `selected` with an allow optionId runs the command; any other answer denies."
    );
    result
}

fn session_update() -> Value {
    let mut params = session(
        json!({"update": object(json!({"sessionUpdate": {"type": "string"}}), &["sessionUpdate"])}),
        &["update"],
    );
    params["description"] = json!(format!("SessionUpdate in full: {ACP}"));
    params
}

fn extension(method: Method) -> (Value, Value) {
    match method {
        Method::SessionList => (
            object(json!({"cwd": {"type": "string"}}), &[]),
            object(
                json!({"sessions": {"type": "array", "items": session_row()}}),
                &["sessions"],
            ),
        ),
        Method::SessionDiff => (
            session(
                json!({"turn": {"oneOf": [{"type": "integer", "minimum": 0}, {"type": "string"}]}}),
                &[],
            ),
            diff(),
        ),
        Method::SessionModel => (
            session(
                json!({"endpoint": {"type": "string"}, "model": {"type": "string"}}),
                &[],
            ),
            empty(),
        ),
        Method::Steer => (
            session(
                json!({"prompt": {"type": "array", "items": reference("ContentBlock")}}),
                &["prompt"],
            ),
            empty(),
        ),
    }
}

fn notice(kind: Note) -> Value {
    let text = json!({"type": "string"});
    match kind {
        Note::Status => {
            let states: Vec<&str> = STATES.iter().map(|(_, name)| *name).collect();
            session(json!({"status": strings(&states)}), &["status"])
        }
        Note::Child => session(
            json!({"childId": text, "kind": text, "title": text}),
            &["childId", "kind", "title"],
        ),
        Note::Usage => session(
            json!({
                "endpoint": text,
                "used": {"type": "integer"},
                "limit": {"type": ["integer", "null"]},
                "cost": {"type": ["number", "null"]},
            }),
            &["endpoint", "used"],
        ),
    }
}

fn session_row() -> Value {
    let text = json!({"type": ["string", "null"]});
    json!({"type": "object", "properties": {
        "id": {"type": "string"},
        "name": text,
        "directory": text,
        "model": text,
        "preview": {"type": "string"},
        "format": {},
    }})
}

fn diff() -> Value {
    let stat = object(
        json!({
            "path": {"type": "string"},
            "added": {"type": "integer"},
            "removed": {"type": "integer"},
            "new": {"type": "boolean"},
        }),
        &["path", "added", "removed", "new"],
    );
    let stats = json!({"type": "array", "items": stat});
    object(
        json!({"base": {"type": "string"}, "session": {"type": "string"}, "files": stats, "strays": stats}),
        &["base", "files"],
    )
}

fn content_block() -> Value {
    json!({
        "type": "object",
        "description": format!("summonr reads `text`, `resource.text`, and `resource_link.uri`. ContentBlock in full: {ACP}"),
        "properties": {
            "type": {"type": "string"},
            "text": {"type": "string"},
            "uri": {"type": "string"},
            "resource": {"type": "object", "properties": {"text": {"type": "string"}}},
        },
        "required": ["type"],
    })
}

fn mcp_server() -> Value {
    let pair = object(
        json!({"name": {"type": "string"}, "value": {"type": "string"}}),
        &["name", "value"],
    );
    let mut server = object(
        json!({
            "name": {"type": "string"},
            "command": {"type": "string"},
            "args": {"type": "array", "items": {"type": "string"}},
            "env": {"type": "array", "items": pair},
        }),
        &["name", "command"],
    );
    server["description"] = json!(format!(
        "Only stdio servers: an entry with `type` (http, sse) answers {}.",
        codes::INVALID
    ));
    server
}

fn config_option() -> Value {
    json!({
        "type": "object",
        "description": format!("SessionConfigOption in full: {ACP}"),
        "properties": {
            "id": strings(&[config::EFFORT]),
            "currentValue": strings(&config::values()),
        },
        "required": ["id", "currentValue"],
        "examples": config::options(None),
    })
}

fn opened() -> Value {
    object(
        json!({
            "sessionId": {"type": "string"},
            "configOptions": {"type": "array", "items": reference("ConfigOption")},
        }),
        &["sessionId", "configOptions"],
    )
}
