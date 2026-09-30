use serde_json::{Value, json};

use crate::support::{Model, Options, Session, Step, calls};

fn server(session: &Session) -> String {
    session.server_env("fixture1", ", MCP_FIXTURE_PROMPTS = '1'", "")
}

fn asked(session: &Session, model: &Model, prompt: &str) -> String {
    let options = Options {
        prompt: Some(prompt),
        ..Options::default()
    };
    session.run_with(model, &server(session), &[], &options);
    model
        .requests()
        .first()
        .and_then(|request| request["messages"].as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter(|message| message["role"] == "user")
        .map(|message| message["content"].to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn a_server_prompt_becomes_the_user_message() {
    let session = Session::new();
    let model = Model::start(vec![Step::Text("ok")]);
    let sent = asked(&session, &model, "/fixture1:review \"fn main() {}\" rust");
    assert!(
        sent.contains("Review this rust code carefully: fn main() {}"),
        "{sent}"
    );
    let received = session.received("fixture1");
    let got = calls(&received, "prompts/get");
    assert_eq!(got.len(), 1, "{received:?}");
    assert_eq!(
        got[0]["params"],
        json!({"name": "review", "arguments": {"code": "fn main() {}", "lang": "rust"}})
    );
}

pub(crate) fn a_prompt_without_its_required_argument_is_refused() {
    let session = Session::new();
    let model = Model::start(vec![Step::Text("ok")]);
    let options = Options {
        prompt: Some("/fixture1:review"),
        ..Options::default()
    };
    let outcome = session.run_with(&model, &server(&session), &[], &options);
    assert!(model.requests().is_empty(), "the model was called");
    assert_ne!(outcome.code, Some(0));
    let text =
        std::fs::read_to_string(session.file("stderr")).unwrap_or_default() + &outcome.stdout;
    assert!(
        text.contains("usage: /fixture1:review <code> [lang]"),
        "{text}"
    );
}

pub(crate) fn a_server_resource_is_attached_to_the_prompt() {
    let session = Session::new();
    let model = Model::start(vec![Step::Text("ok")]);
    let sent = asked(&session, &model, "summarise @fixture1:note://today please");
    assert!(sent.contains("the note kept at note://today"), "{sent}");
    let received = session.received("fixture1");
    let read = calls(&received, "resources/read");
    assert_eq!(read.len(), 1, "{received:?}");
    assert_eq!(read[0]["params"], json!({"uri": "note://today"}));
}

pub(crate) fn a_changed_prompt_list_is_fetched_again() {
    let session = Session::new();
    let model = Model::start(vec![
        Step::Call("fixture1__reprompt", json!({})),
        Step::Text("ok"),
    ]);
    session.run(&model, &server(&session), &[]);
    let received = session.received("fixture1");
    let changed = received
        .iter()
        .position(|frame| frame["params"]["name"] == "reprompt")
        .unwrap_or_else(|| panic!("{received:?}"));
    assert_eq!(
        calls(&received[changed..], "prompts/list").len(),
        1,
        "{received:?}"
    );
}

pub(crate) fn a_form_is_declined_without_a_user() {
    let session = Session::new();
    let model = Model::start(vec![
        Step::Call("fixture1__ask", json!({})),
        Step::Text("ok"),
    ]);
    session.run(&model, &server(&session), &[]);
    let received = session.received("fixture1");
    let hello = calls(&received, "initialize");
    assert_eq!(
        hello[0]["params"]["capabilities"]["elicitation"],
        json!({"form": {}})
    );
    let answer: Vec<&Value> = received
        .iter()
        .filter(|frame| frame["id"] == "fixture-ask")
        .collect();
    assert_eq!(answer.len(), 1, "{received:?}");
    assert_eq!(answer[0]["result"], json!({"action": "decline"}));
}
