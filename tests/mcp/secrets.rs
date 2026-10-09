use serde_json::json;

use crate::support::{Model, Session, Step};

const STRIPE: &str = "fake-stripe-CANARY0123456789";
const KEY_LINE: &str = "MIIEvQIBADANBgkqhkiG9w0BAQEFAASC";
const NPM: &str = "npm_CANARY0123456789abcdef";

#[cfg(unix)]
pub(crate) fn secret_values_never_reach_the_model_but_commands_still_load_them() {
    let session = Session::new();
    let work = session.work.path();
    std::fs::create_dir_all(work.join("apps/web")).unwrap();
    std::fs::write(
        work.join("apps/web/.env"),
        format!("PORT=3000\nSTRIPE_KEY=\"{STRIPE}\"\n"),
    )
    .unwrap();
    std::fs::write(
        work.join("notes.txt"),
        format!("-----BEGIN PRIVATE KEY-----\n{KEY_LINE}\n-----END PRIVATE KEY-----\nafter\n"),
    )
    .unwrap();
    std::fs::write(
        work.join(".npmrc"),
        format!("//registry.npmjs.org/:_authToken={NPM}\n"),
    )
    .unwrap();
    let model = Model::start(vec![
        Step::Call("run_command", json!({"command": "grep -r STRIPE ."})),
        Step::Call(
            "run_command",
            json!({"command": "cat apps/web/.env notes.txt .npmrc"}),
        ),
        Step::Call("read", json!({"path": "apps/web/.env"})),
        Step::Call(
            "run_command",
            json!({"command": format!(". apps/web/.env && test \"$STRIPE_KEY\" = {STRIPE} && echo loaded")}),
        ),
        Step::Call("search", json!({"pattern": "STRIPE"})),
        Step::Text("done"),
    ]);
    session.run(&model, "", &["--no-verify"]);
    let results = model.tool_results();
    assert_eq!(results.len(), 5, "{results:?}");
    for result in &results {
        assert!(!result.contains(STRIPE), "{results:?}");
        assert!(!result.contains(KEY_LINE), "{results:?}");
        assert!(!result.contains(NPM), "{results:?}");
    }
    assert!(
        results[0].contains("STRIPE_KEY=\\\"[secret]"),
        "{results:?}"
    );
    assert!(results[1].contains("PORT=3000"), "{results:?}");
    assert!(results[1].contains("after"), "{results:?}");
    assert!(results[3].contains("loaded"), "{results:?}");
    assert!(!results[4].contains(".env"), "{results:?}");
}
