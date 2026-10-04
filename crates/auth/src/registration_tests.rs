use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::*;

struct Memory(Mutex<std::collections::BTreeMap<String, String>>);

impl Store for Memory {
    fn get(&self, name: &str) -> Result<Option<String>> {
        Ok(self.0.lock().unwrap().get(name).cloned())
    }
    fn set(&self, name: &str, secret: &str) -> Result<()> {
        self.0
            .lock()
            .unwrap()
            .insert(name.to_string(), secret.to_string());
        Ok(())
    }
    fn forget(&self, name: &str) -> Result<bool> {
        Ok(self.0.lock().unwrap().remove(name).is_some())
    }
}

async fn token_endpoint(reply: &'static str) -> (String, Arc<Mutex<String>>) {
    let issuer = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("the mock token endpoint binds");
    let url = format!("http://{}/token", issuer.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(String::new()));
    let posted = seen.clone();
    tokio::spawn(async move {
        let (mut stream, _) = issuer.accept().await.expect("the mock takes a request");
        let mut seen = Vec::new();
        loop {
            let mut raw = vec![0u8; 4096];
            let read = tokio::io::AsyncReadExt::read(&mut stream, &mut raw)
                .await
                .expect("the mock reads");
            seen.extend_from_slice(&raw[..read]);
            let text = String::from_utf8_lossy(&seen).to_string();
            let Some((head, body)) = text.split_once("\r\n\r\n") else {
                continue;
            };
            let wanted: usize = head
                .lines()
                .find_map(|line| {
                    line.split_once(':')
                        .filter(|(key, _)| key.eq_ignore_ascii_case("content-length"))
                        .and_then(|(_, value)| value.trim().parse().ok())
                })
                .unwrap_or_default();
            if read == 0 || body.len() >= wanted {
                posted.lock().unwrap().push_str(&text);
                break;
            }
        }
        let head = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{reply}",
            reply.len()
        );
        tokio::io::AsyncWriteExt::write_all(&mut stream, head.as_bytes())
            .await
            .expect("the mock answers");
    });
    (url, seen)
}

fn registering(token_url: String) -> Spec {
    Spec {
        token_url,
        client_id: "dynamic_agent_client".into(),
        grant: Grant::AuthorizationCode,
        authorize_url: Some("https://example.invalid/authorize".into()),
        redirect_uri: Some("http://127.0.0.1:0/auth/callback".into()),
        token_params: std::collections::BTreeMap::from([(
            "resource".to_string(),
            "https://api.example.invalid/v1".to_string(),
        )]),
        ..Spec::default()
    }
}

#[tokio::test]
async fn a_redirect_that_issues_a_client_id_is_spent_on_the_exchange_and_kept() {
    let (token_url, seen) =
        token_endpoint(r#"{"access_token":"at","refresh_token":"rt","expires_in":3600}"#).await;
    let kept: Arc<dyn Store> = Arc::new(Memory(Mutex::new(Default::default())));
    let auth = Auth::new(
        Method::Login(Box::new(registering(token_url))),
        Scheme::Bearer,
        "plan",
        kept.clone(),
    );
    let http = http(Duration::from_secs(5), Duration::from_secs(5)).unwrap();

    let lines = Arc::new(Mutex::new(Vec::new()));
    let shown = lines.clone();
    let visitor = tokio::spawn(async move {
        let link: String = loop {
            if let Some(line) = shown.lock().unwrap().first().cloned() {
                break line;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        let asked = url::Url::parse(link.trim_start_matches("open ")).unwrap();
        let query: std::collections::BTreeMap<_, _> = asked.query_pairs().into_owned().collect();
        let back = url::Url::parse(query.get("redirect_uri").unwrap()).unwrap();
        assert_eq!(back.path(), "/auth/callback", "the provider's path is kept");
        assert_ne!(back.port(), Some(0), "port 0 means a free port, not port 0");
        assert!(query.contains_key("nonce"), "every attempt carries a nonce");
        let state = query.get("state").unwrap();
        let mut stream = tokio::net::TcpStream::connect(back.socket_addrs(|| None).unwrap()[0])
            .await
            .expect("the browser reaches the catcher on the substituted port");
        let get = format!(
            "GET /auth/callback?code=xyz&state={state}&client_id=oaiapp_1 HTTP/1.1\r\nhost: x\r\n\r\n"
        );
        tokio::io::AsyncWriteExt::write_all(&mut stream, get.as_bytes())
            .await
            .unwrap();
        back.to_string()
    });

    let heard = lines.clone();
    let mut prompt = Prompt::telling(move |line| heard.lock().unwrap().push(line.to_string()));
    auth.login(&http, &mut prompt)
        .await
        .expect("the login completes");
    let redirect = visitor.await.unwrap();

    let form = seen.lock().unwrap().clone();
    assert!(
        form.contains("client_id=oaiapp_1"),
        "the exchange spends the issued id, got {form}"
    );
    assert!(
        form.contains(&format!(
            "redirect_uri={}",
            url::form_urlencoded::byte_serialize(redirect.as_bytes()).collect::<String>()
        )),
        "the exchange names the redirect the browser used, got {form}"
    );
    assert!(
        form.contains("resource=https"),
        "token_params ride the exchange, got {form}"
    );
    let stored = kept.get("plan").unwrap().expect("a token was stored");
    assert!(
        stored.contains(r#""client_id":"oaiapp_1""#),
        "the issued id is kept for later refreshes, got {stored}"
    );
}

#[tokio::test]
async fn a_refresh_spends_the_issued_client_id_and_the_token_params() {
    let (token_url, seen) =
        token_endpoint(r#"{"access_token":"next","refresh_token":"rt2","expires_in":3600}"#).await;
    let kept: Arc<dyn Store> = Arc::new(Memory(Mutex::new(Default::default())));
    kept.set(
        "plan",
        r#"{"access_token":"old","refresh_token":"rt","expires_at":1,"client_id":"oaiapp_1"}"#,
    )
    .unwrap();
    let mut spec = registering(token_url);
    spec.scope = Some("openid offline_access".into());
    let auth = Auth::new(Method::Login(Box::new(spec)), Scheme::Bearer, "plan", kept);
    let http = http(Duration::from_secs(5), Duration::from_secs(5)).unwrap();

    let live = auth.token(&http).await.expect("the refresh succeeds");
    assert_eq!(live, "next");
    let form = seen.lock().unwrap().clone();
    assert!(form.contains("grant_type=refresh_token"), "got {form}");
    assert!(
        form.contains("client_id=oaiapp_1"),
        "the issued id, not the seed, got {form}"
    );
    assert!(
        form.contains("resource=https"),
        "token_params ride the refresh, got {form}"
    );
    assert!(
        !form.contains("scope="),
        "a refresh keeps the original grant, got {form}"
    );
}

#[tokio::test]
async fn a_sign_in_without_the_plan_scope_is_refused() {
    let (token_url, _seen) = token_endpoint(
        r#"{"access_token":"at","refresh_token":"rt","expires_in":3600,"scope":"openid profile"}"#,
    )
    .await;
    let kept: Arc<dyn Store> = Arc::new(Memory(Mutex::new(Default::default())));
    let mut spec = registering(token_url);
    spec.required_scope = Some("chatgpt.tokens.use.direct".into());
    let auth = Auth::new(
        Method::Login(Box::new(spec)),
        Scheme::Bearer,
        "plan",
        kept.clone(),
    );
    let http = http(Duration::from_secs(5), Duration::from_secs(5)).unwrap();
    let lines = Arc::new(Mutex::new(Vec::new()));
    let shown = lines.clone();
    tokio::spawn(async move {
        let link: String = loop {
            if let Some(line) = shown.lock().unwrap().first().cloned() {
                break line;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        let asked = url::Url::parse(link.trim_start_matches("open ")).unwrap();
        let query: std::collections::BTreeMap<_, _> = asked.query_pairs().into_owned().collect();
        let back = url::Url::parse(query.get("redirect_uri").unwrap()).unwrap();
        let state = query.get("state").unwrap();
        let mut stream = tokio::net::TcpStream::connect(back.socket_addrs(|| None).unwrap()[0])
            .await
            .unwrap();
        let get = format!("GET /auth/callback?code=xyz&state={state} HTTP/1.1\r\nhost: x\r\n\r\n");
        tokio::io::AsyncWriteExt::write_all(&mut stream, get.as_bytes())
            .await
            .unwrap();
    });
    let heard = lines.clone();
    let mut prompt = Prompt::telling(move |line| heard.lock().unwrap().push(line.to_string()));
    let error = auth
        .login(&http, &mut prompt)
        .await
        .expect_err("a token without plan usage must not count as signed in");
    assert!(
        error.to_string().contains("chatgpt.tokens.use.direct"),
        "the message names the missing scope, got {error}"
    );
    assert!(kept.get("plan").unwrap().is_none(), "nothing is stored");
}
