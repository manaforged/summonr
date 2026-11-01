use super::{App, Error, Event, Msg, Result, Scenario};
use crate::child::Access;

const AGENTS: [(&str, &str, &str); 3] = [
    ("capture-api", "Read the API docs", "docs/api.md"),
    ("capture-readme", "Review the README", "README.md"),
    (
        "capture-checks",
        "Find contributor checks",
        "CONTRIBUTING.md",
    ),
];

pub(super) fn seed(app: &App, scenario: Scenario) {
    match scenario {
        Scenario::Help => {
            app.local("/help", None);
        }
        Scenario::Agents | Scenario::AgentsFailed => agents(app, scenario),
        Scenario::Idle | Scenario::Stream | Scenario::Resize => {}
    }
}

fn agents(app: &App, scenario: Scenario) {
    app.update(Msg::User(String::from(
        "Run three lightweight, read-only agents.",
    )));
    app.update(Msg::Event(Event::Reasoning {
        text: String::from("I will inspect the documentation without editing files."),
    }));
    app.update(Msg::Event(Event::ToolCall {
        id: String::from("capture-tasks"),
        name: String::from("task"),
        arguments: String::from("{}"),
    }));
    for (id, assignment, path) in AGENTS {
        for event in [
            Event::Spawned {
                assignment: assignment.to_owned(),
                access: Access::Research,
                call: String::from("capture-tasks"),
                background: true,
                workspace: String::new(),
                agent: String::new(),
            },
            Event::ToolCall {
                id: format!("{id}-read"),
                name: String::from("read"),
                arguments: serde_json::json!({"path": path}).to_string(),
            },
        ] {
            child(app, id, event);
        }
    }
    if scenario == Scenario::AgentsFailed {
        child(
            app,
            AGENTS[0].0,
            Event::Aborted {
                reason: String::from("Documentation check failed"),
                code: Default::default(),
            },
        );
        app.model_mut(|model| {
            for agent in &mut model.agents {
                if agent.done {
                    agent.secs = Some(0);
                }
            }
        });
    }
}

fn child(app: &App, id: &str, event: Event) {
    app.update(Msg::Event(Event::Child {
        id: id.to_owned(),
        event: Box::new(event),
    }));
}

pub(super) fn verify(scenario: Scenario, text: &str) -> Result<()> {
    let required: &[&str] = match scenario {
        Scenario::Help => &["Help", "find", "esc close"],
        Scenario::Agents => &[
            "Read the API docs",
            "Review the README",
            "Find contributor checks",
        ],
        Scenario::AgentsFailed => &[
            "failed after",
            "Review the README",
            "Find contributor checks",
        ],
        Scenario::Idle | Scenario::Stream | Scenario::Resize => &[],
    };
    for content in required {
        if !text.contains(content) {
            return Err(Error::Tool(format!(
                "UI capture missing expected content: {content}"
            )));
        }
    }
    Ok(())
}
