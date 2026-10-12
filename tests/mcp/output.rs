use serde_json::json;

use crate::support::{Model, Options, Session, Step};

pub(crate) fn large_output_is_cut_for_the_model_and_kept_whole_in_a_file() {
    let session = Session::new();
    let model = Model::start(vec![
        Step::Call("fixture1__big", json!({"bytes": 200_000})),
        Step::Text("ok"),
    ]);
    session.run(&model, &session.server("fixture1", ""), &[]);
    let result = &model.tool_results()[0];
    assert!(
        result.len() < 60_000,
        "{} bytes reached the model",
        result.len()
    );
    let kept = model.requests().last().unwrap()["kept"].clone();
    assert_eq!(kept[0][1], json!(200_000), "{kept}");
}

pub(crate) fn a_reply_over_the_frame_cap_is_dropped_and_the_session_goes_on() {
    let session = Session::new();
    let model = Model::start(vec![
        Step::Call("fixture1__big", json!({"bytes": 5 * 1024 * 1024})),
        Step::Call("fixture1__echo", json!({"text": "after"})),
        Step::Text("ok"),
    ]);
    session.run(&model, &session.server("fixture1", ""), &[]);
    let results = model.tool_results();
    assert!(results[0].contains("over 4 MiB"), "{}", head(&results[0]));
    assert!(results[1].contains("after"), "{results:?}");
}

pub(crate) fn an_error_reply_is_cut_to_the_budget() {
    let session = Session::new();
    let model = Model::start(vec![
        Step::Call("fixture1__boom", json!({"bytes": 200_000})),
        Step::Text("ok"),
    ]);
    session.run(&model, &session.server("fixture1", ""), &[]);
    let result = &model.tool_results()[0];
    assert!(
        result.len() < 60_000,
        "{} bytes reached the model",
        result.len()
    );
    assert!(result.contains("[full output: "), "{}", head(result));
}

pub(crate) fn output_that_cannot_be_saved_is_still_cut_and_returned() {
    let session = Session::new();
    let model = Model::start(vec![
        Step::Call("fixture1__big", json!({"bytes": 200_000})),
        Step::Text("ok"),
    ]);
    let options = Options {
        env: &[
            ("TMPDIR", "/nonexistent/summonr-proof"),
            ("TMP", "/nonexistent/summonr-proof"),
            ("TEMP", "/nonexistent/summonr-proof"),
        ],
        ..Options::default()
    };
    session.run_with(&model, &session.server("fixture1", ""), &[], &options);
    let result = &model.tool_results()[0];
    assert!(
        result.contains("[full output not saved"),
        "{}",
        head(result)
    );
    assert!(
        result.len() < 60_000,
        "{} bytes reached the model",
        result.len()
    );
}

pub(crate) fn an_image_reaches_a_model_that_takes_images() {
    let session = Session::new();
    let model = Model::start(vec![
        Step::Call("fixture1__image", json!({})),
        Step::Text("ok"),
    ]);
    let options = Options {
        images: true,
        ..Options::default()
    };
    session.run_with(&model, &session.server("fixture1", ""), &[], &options);
    let last = model.requests().last().unwrap().to_string();
    assert!(
        last.contains("data:image/png;base64,iVBOR"),
        "{}",
        head(&last)
    );
}

pub(crate) fn an_image_is_withheld_from_a_model_without_images() {
    let session = Session::new();
    let model = Model::start(vec![
        Step::Call("fixture1__image", json!({})),
        Step::Text("ok"),
    ]);
    session.run(&model, &session.server("fixture1", ""), &[]);
    let last = model.requests().last().unwrap().to_string();
    assert!(!last.contains("data:image/"), "{}", head(&last));
    assert!(
        model.tool_results()[0].contains("image(s) omitted"),
        "{:?}",
        model.tool_results()
    );
}

pub(crate) fn an_image_whose_data_does_not_match_its_type_is_withheld() {
    let session = Session::new();
    let model = Model::start(vec![
        Step::Call("fixture1__image", json!({"mime": "image/svg+xml"})),
        Step::Text("ok"),
    ]);
    let options = Options {
        images: true,
        ..Options::default()
    };
    session.run_with(&model, &session.server("fixture1", ""), &[], &options);
    let last = model.requests().last().unwrap().to_string();
    assert!(!last.contains("data:image/"), "{}", head(&last));
}

pub(crate) fn a_server_error_line_is_shown_without_secrets() {
    let session = Session::new();
    let config = session.server_env(
        "fixture1",
        ", MCP_FIXTURE_SECRET = 'hunter2-proof-value'",
        "",
    );
    let model = Model::start(vec![
        Step::Call("fixture1__leak", json!({})),
        Step::Text("ok"),
    ]);
    session.run(&model, &config, &[]);
    let result = &model.tool_results()[0];
    assert!(result.contains("token [secret]"), "{result}");
    assert!(!result.contains("hunter2"), "{result}");
}

fn head(text: &str) -> String {
    text.chars().take(300).collect()
}

pub(crate) fn a_json_run_starts_with_its_session_id() {
    let session = Session::new();
    let outcome = session.run(&Model::start(vec![Step::Text("ok")]), "", &["--json"]);
    let first: serde_json::Value =
        serde_json::from_str(outcome.stdout.lines().next().unwrap_or_default())
            .unwrap_or_else(|_| panic!("{}", outcome.stdout));
    assert_eq!(first["type"], "session", "{}", outcome.stdout);
    let id = first["id"].as_str().unwrap_or_default();
    assert!(id.len() >= 12, "{first}");
    let stderr = crate::support::read(&session.file("stderr"));
    assert!(stderr.contains(&id[..12]), "{id}: {stderr}");
}
