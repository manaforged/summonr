use super::*;

fn browsing(files: &[(&str, &str)]) -> (tempfile::TempDir, ToolRegistry) {
    let (dir, mut tools) = workspace(files);
    tools.set_browser(true);
    (dir, tools)
}

async fn call(tools: &ToolRegistry, arguments: serde_json::Value) -> crate::error::Result<String> {
    let call = ToolCall {
        id: "t1".to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            checked: Default::default(),
            name: "browser".to_string(),
            arguments: arguments.to_string(),
        },
        continuation: None,
    };
    tools
        .execute_outcome(&call)
        .await
        .map(|outcome| outcome.text)
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap()
}

#[test]
fn the_browser_refuses_private_hosts_and_plain_http_before_launching() {
    let (_dir, tools) = browsing(&[]);
    runtime().block_on(async {
        for url in [
            "http://127.0.0.1/",
            "https://127.0.0.1/",
            "file:///etc/passwd",
        ] {
            let refused = call(
                &tools,
                serde_json::json!({"action": "navigate", "url": url}),
            )
            .await;
            let message = refused.expect_err(url).to_string();
            assert!(
                message.contains("not allowed") || message.contains("https"),
                "{url}: {message}"
            );
        }
    });
}

#[test]
fn a_short_javascript_wall_is_detected_and_a_real_page_is_not() {
    let wall = crate::browser::wall::WALL;
    assert!(wall.body("Just a moment...", "<title>Just a moment...</title>"));
    assert!(wall.body(
        "",
        "<noscript>Please enable JavaScript to continue</noscript>"
    ));
    let article = "word ".repeat(1000);
    assert!(!wall.body(
        &article,
        &format!("<p>{article}</p><noscript>enable javascript</noscript>")
    ));
    assert!(wall.status(403));
    assert!(!wall.status(200));
}

#[test]
#[ignore = "launches the installed Chrome and reaches example.com"]
fn the_browser_navigates_snapshots_and_clicks_a_ref() {
    let (_dir, tools) = browsing(&[]);
    runtime().block_on(async {
        let opened = call(
            &tools,
            serde_json::json!({"action": "navigate", "url": "https://example.com/"}),
        )
        .await
        .unwrap();
        assert!(opened.contains("url: https://example.com/"), "{opened}");
        let snapshot = call(&tools, serde_json::json!({"action": "snapshot"}))
            .await
            .unwrap();
        assert!(snapshot.contains("link \"Learn more\""), "{snapshot}");
        let line = snapshot
            .lines()
            .find(|line| line.contains("- link"))
            .expect("a link");
        let start = line.find("[ref=").expect("ref") + 5;
        let end = line[start..].find(']').expect("ref end") + start;
        let clicked = call(
            &tools,
            serde_json::json!({"action": "click", "target": &line[start..end]}),
        )
        .await
        .unwrap();
        assert!(clicked.contains("from: https://example.com/"), "{clicked}");
        call(&tools, serde_json::json!({"action": "close"}))
            .await
            .unwrap();
    });
}

#[test]
#[ignore = "launches the installed Chrome and reaches httpbin.org"]
fn a_login_cookie_survives_summonr_exiting() {
    let set = "https://httpbin.org/response-headers?Set-Cookie=kept%3Dyes%3B%20Max-Age%3D86400%3B%20Path%3D%2F";
    let (_first_dir, first) = browsing(&[]);
    runtime().block_on(async {
        call(
            &first,
            serde_json::json!({"action": "navigate", "url": set}),
        )
        .await
        .expect("set the cookie");
    });
    drop(first);
    let (_second_dir, second) = browsing(&[]);
    runtime().block_on(async {
        call(
            &second,
            serde_json::json!({"action": "navigate", "url": "https://httpbin.org/cookies"}),
        )
        .await
        .expect("open cookies");
        let read = call(&second, serde_json::json!({"action": "read"}))
            .await
            .expect("read");
        assert!(read.contains("kept"), "{read}");
        call(&second, serde_json::json!({"action": "close"}))
            .await
            .expect("close");
    });
}

#[test]
#[ignore = "launches the installed Chrome and reaches httpbin.org"]
fn a_redirect_to_a_private_host_returns_nothing_from_it() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut stream = stream;
            let mut buf = [0_u8; 2048];
            drop(stream.read(&mut buf));
            let body = "<title>private</title><p>SECRET-METADATA-TOKEN</p>";
            drop(write!(
                stream,
                "HTTP/1.1 200 OK\r\ncontent-type: text/html\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            ));
        }
    });
    let target = format!("http%3A%2F%2F127.0.0.1%3A{port}%2F");
    let (_dir, tools) = browsing(&[]);
    runtime().block_on(async {
        let landed = call(
            &tools,
            serde_json::json!({"action": "navigate", "url": format!("https://httpbin.org/redirect-to?url={target}")}),
        )
        .await
        .expect("navigate");
        let read = call(&tools, serde_json::json!({"action": "read"}))
            .await
            .expect("read");
        for text in [&landed, &read] {
            assert!(!text.contains("SECRET-METADATA-TOKEN"), "{text}");
            assert!(!text.contains("private"), "{text}");
        }
        assert!(landed.contains("blocked"), "{landed}");
        call(&tools, serde_json::json!({"action": "close"}))
            .await
            .expect("close");
    });
}
