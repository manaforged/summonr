use base64::Engine;
use serde_json::json;

use crate::support::{Model, Options, Session, Step};

const PAGE: &str = r#"<html><title>Tool qualification</title><body><noscript>Enable JavaScript</noscript><div id="app"></div><script>
document.getElementById('app').innerHTML=`<h1>JS READY</h1><label>Name<input id="name"></label><select id="kind"><option value="one">One</option><option value="two">Two</option></select><label>Agree<input type="checkbox" id="agree"></label><input type="file" id="file"><button id="show">Show</button><p id="result"></p>`;
document.getElementById('show').onclick=()=>{document.getElementById('result').textContent='Result: '+document.getElementById('name').value+'|'+document.getElementById('kind').value+'|'+document.getElementById('agree').checked+'|'+document.getElementById('file').files[0].name};
</script></body></html>"#;

pub(crate) fn live_browser_round_trips_through_the_model() {
    let session = Session::new();
    std::fs::write(session.work.path().join("payload.txt"), "upload proof\n")
        .expect("upload fixture");
    let encoded = base64::engine::general_purpose::STANDARD.encode(PAGE);
    let path: String = url::form_urlencoded::byte_serialize(encoded.as_bytes()).collect();
    let url = format!("https://httpbin.org/base64/{path}");
    let calls = [
        ("web", json!({"url":url})),
        ("web", json!({"url":url,"format":"html"})),
        ("web", json!({"url":url,"format":"text"})),
        ("browser", json!({"action":"snapshot"})),
        (
            "browser",
            json!({"action":"fill","target":"#name","text":"Alice"}),
        ),
        (
            "browser",
            json!({"action":"type","target":"#name","text":"!"}),
        ),
        (
            "browser",
            json!({"action":"press","target":"#name","key":"Backspace"}),
        ),
        (
            "browser",
            json!({"action":"select","target":"#kind","by":{"value":["two"]}}),
        ),
        (
            "browser",
            json!({"action":"check","target":"#agree","checked":true}),
        ),
        (
            "browser",
            json!({"action":"upload","target":"#file","files":["payload.txt"]}),
        ),
        ("browser", json!({"action":"hover","target":"#show"})),
        (
            "browser",
            json!({"action":"click","target":"#show","options":{"button":"left","count":1},"report":"full"}),
        ),
        (
            "browser",
            json!({"action":"wait","for":{"text":"Result: Alice|two|true|payload.txt"}}),
        ),
        ("browser", json!({"action":"scroll","delta_y":120})),
        ("browser", json!({"action":"read","format":"text"})),
        ("browser", json!({"action":"screenshot","full_page":true})),
        (
            "browser",
            json!({"action":"tab","new":"https://example.com/"}),
        ),
        ("browser", json!({"action":"tabs"})),
        ("browser", json!({"action":"back"})),
        ("browser", json!({"action":"forward"})),
        ("browser", json!({"action":"reload"})),
        ("browser", json!({"action":"close"})),
    ];
    let count = calls.len();
    let mut script: Vec<_> = calls
        .into_iter()
        .map(|(name, args)| Step::Call(name, args))
        .collect();
    script.push(Step::Text("done"));
    let model = Model::start(script);
    let outcome = session.run_with(
        &model,
        "[modules]\nsearch = true\nbrowser = true\n",
        &["--no-verify", "--json"],
        &Options::default(),
    );
    assert_eq!(outcome.code, Some(0), "{}", outcome.stdout);
    assert_eq!(model.requests().len(), count + 1);
    let results: Vec<String> = model
        .tool_results()
        .iter()
        .map(|result| serde_json::from_str(result).expect("text result"))
        .collect();
    assert_eq!(results.len(), count, "{results:?}");
    assert!(
        results[0].contains("rendered in the browser") && results[0].contains("# JS READY"),
        "{}",
        results[0]
    );
    assert!(results[1].contains("<h1>JS READY</h1>"), "{}", results[1]);
    assert!(
        !results[2].contains("# JS READY") && results[2].contains("JS READY"),
        "{}",
        results[2]
    );
    assert!(results[3].contains("textbox"), "{}", results[3]);
    assert!(
        results[14].contains("Result: Alice|two|true|payload.txt"),
        "{}",
        results[14]
    );
    let shot = results[15]
        .strip_prefix("screenshot: ")
        .expect("screenshot path");
    let bytes = std::fs::read(shot).expect("PNG exists during the session");
    assert_eq!(bytes.get(..8), Some(b"\x89PNG\r\n\x1a\n".as_slice()));
    assert!(
        results[17].contains("Example Domain") && results[17].contains("Tool qualification"),
        "{}",
        results[17]
    );
    assert!(results[18].contains("about:blank"), "{}", results[18]);
    assert!(
        results[19].contains("https://example.com/"),
        "{}",
        results[19]
    );
    assert_eq!(results[21], "browser closed");
    let events: Vec<serde_json::Value> = outcome
        .stdout
        .lines()
        .map(|line| serde_json::from_str(line).expect("JSON event"))
        .filter(|event: &serde_json::Value| event["type"] == "tool_result")
        .collect();
    assert_eq!(events.len(), count);
    for event in events {
        assert_eq!(event["success"], true, "{event}");
    }
    #[cfg(unix)]
    assert!(
        crate::support::survivors(
            &session
                .home
                .path()
                .join("browser/profiles")
                .to_string_lossy()
        )
        .is_empty(),
        "Chrome remains after close"
    );
}
