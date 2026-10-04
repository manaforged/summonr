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

fn store() -> Arc<dyn Store> {
    Arc::new(Memory(Mutex::new(std::collections::BTreeMap::new())))
}

async fn whole(stream: &mut tokio::net::TcpStream) -> String {
    let mut seen = Vec::new();
    loop {
        let mut raw = vec![0u8; 4096];
        let read = tokio::io::AsyncReadExt::read(stream, &mut raw)
            .await
            .expect("the mock reads the request");
        if read == 0 {
            break;
        }
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
        if body.len() >= wanted {
            return text;
        }
    }
    String::from_utf8_lossy(&seen).to_string()
}

#[test]
fn a_key_endpoint_needs_no_login_and_is_ready() {
    let auth = Auth::new(Method::Key("k".into()), Scheme::Bearer, "zai", store());
    assert!(!auth.is_login());
    assert!(auth.ready().unwrap());
}

#[test]
fn a_login_endpoint_is_not_ready_until_something_is_stored() {
    let kept = store();
    let spec = Spec {
        token_url: "https://example.invalid/token".into(),
        client_id: "id".into(),
        grant: Grant::DeviceCode,
        device_url: Some("https://example.invalid/device".into()),
        ..Spec::default()
    };
    let auth = Auth::new(
        Method::Login(Box::new(spec)),
        Scheme::Bearer,
        "copilot",
        kept.clone(),
    );
    assert!(auth.is_login());
    assert!(!auth.ready().unwrap());
    kept.set(
        "copilot",
        r#"{"access_token":"t","refresh_token":"r","expires_at":9999999999}"#,
    )
    .unwrap();
    assert!(auth.ready().unwrap());
}

#[test]
fn each_wire_puts_the_credential_where_that_wire_wants_it() {
    assert_eq!(Scheme::default(), Scheme::Bearer);
    for scheme in [Scheme::Bearer, Scheme::ApiKey, Scheme::GoogleKey] {
        let auth = Auth::new(Method::Key("k".into()), scheme, "x", store());
        assert_eq!(auth.scheme(), scheme);
    }
}

#[test]
fn a_login_spec_states_what_it_is_missing() {
    let device = Spec {
        grant: Grant::DeviceCode,
        ..Spec::default()
    };
    assert!(
        device
            .check()
            .unwrap_err()
            .to_string()
            .contains("device_url"),
        "a device login without device_url must say so"
    );
    let refreshing = Spec::default();
    assert!(
        refreshing
            .check()
            .unwrap_err()
            .to_string()
            .contains("refresh_token")
    );
}

#[test]
fn a_stored_token_round_trips_through_the_store_format() {
    let issued =
        oauth::parse_stored(r#"{"access_token":"a","refresh_token":"r","expires_at":9999999999}"#)
            .expect("a stored token parses");
    assert_eq!(issued.access, "a");
    assert_eq!(issued.refresh.as_deref(), Some("r"));
    assert!(issued.fresh(), "a token far in the future is usable");
    assert!(oauth::parse_stored("not json").is_none());
}

#[tokio::test]
async fn a_paste_back_login_exchanges_the_code_the_user_typed() {
    let issuer = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("the mock token endpoint binds");
    let token_url = format!(
        "http://{}/token",
        issuer.local_addr().expect("the mock has an address")
    );
    let seen = Arc::new(Mutex::new(String::new()));
    let posted = seen.clone();
    let served = tokio::spawn(async move {
        let (mut stream, _) = issuer.accept().await.expect("the mock takes the exchange");
        let seen = whole(&mut stream).await;
        posted.lock().unwrap().push_str(&seen);
        let body = r#"{"access_token":"oat","expires_in":3600}"#;
        let reply = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        tokio::io::AsyncWriteExt::write_all(&mut stream, reply.as_bytes())
            .await
            .expect("the mock answers");
    });

    let kept = store();
    let spec = Spec {
        token_url,
        client_id: "cli".into(),
        grant: Grant::AuthorizationCode,
        authorize_url: Some("https://provider.invalid/oauth/authorize".into()),
        redirect_uri: Some("https://provider.invalid/oauth/code/callback".into()),
        token_style: TokenStyle::Json,
        ..Spec::default()
    };
    let auth = Auth::new(Method::Login(Box::new(spec)), Scheme::Bearer, "paste", kept);
    let http = http(Duration::from_secs(5), Duration::from_secs(5)).expect("a client builds");

    let lines = Arc::new(Mutex::new(Vec::<String>::new()));
    let heard = lines.clone();
    let answering = lines.clone();
    let mut prompt = Prompt::new(
        move |line| heard.lock().unwrap().push(line.to_string()),
        move |_| {
            let answering = answering.clone();
            Box::pin(async move {
                let link = answering.lock().unwrap().first().cloned()?;
                let asked = url::Url::parse(link.trim_start_matches("open ")).ok()?;
                let state = asked
                    .query_pairs()
                    .find(|(key, _)| key == "state")
                    .map(|(_, value)| value.into_owned())?;
                Some(format!("pasted-code#{state}"))
            })
        },
    );

    auth.login(&http, &mut prompt)
        .await
        .expect("the paste-back login completes");
    served.await.expect("the mock finishes");

    let sent = seen.lock().unwrap().clone();
    let body = sent
        .split("\r\n\r\n")
        .nth(1)
        .unwrap_or_default()
        .to_string();
    let json: serde_json::Value = serde_json::from_str(&body)
        .unwrap_or_else(|error| panic!("json body, got {body}: {error}"));
    assert_eq!(json["grant_type"], "authorization_code");
    assert_eq!(json["code"], "pasted-code", "the # state must be stripped");
    assert!(
        json.get("code_verifier")
            .is_some_and(serde_json::Value::is_string),
        "the exchange still spends a verifier: {json}"
    );
    assert_eq!(
        auth.token(&http).await.expect("the token is live"),
        "oat",
        "no local port was opened and the token still landed"
    );
}

#[tokio::test]
async fn a_browser_login_completes_a_pkce_round_trip() {
    let issuer = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("the mock token endpoint binds");
    let token_url = format!(
        "http://{}/token",
        issuer.local_addr().expect("the mock has an address")
    );
    let seen = Arc::new(Mutex::new(String::new()));
    let posted = seen.clone();
    let served = tokio::spawn(async move {
        let (mut stream, _) = issuer.accept().await.expect("the mock takes the exchange");
        let seen = whole(&mut stream).await;
        posted.lock().unwrap().push_str(&seen);
        let body = r#"{"access_token":"at","refresh_token":"rt","expires_in":3600}"#;
        let reply = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        tokio::io::AsyncWriteExt::write_all(&mut stream, reply.as_bytes())
            .await
            .expect("the mock answers");
    });

    let kept = store();
    let spec = Spec {
        token_url,
        client_id: "cli".into(),
        grant: Grant::AuthorizationCode,
        authorize_url: Some("https://example.invalid/authorize".into()),
        scope: Some("offline_access".into()),
        ..Spec::default()
    };
    let auth = Auth::new(
        Method::Login(Box::new(spec)),
        Scheme::Bearer,
        "mock",
        kept.clone(),
    );
    let http = http(Duration::from_secs(5), Duration::from_secs(5)).expect("a client builds");

    let asked = Arc::new(Mutex::new(Vec::new()));
    let lines = asked.clone();
    let visitor = tokio::spawn(async move {
        let link: String = loop {
            if let Some(line) = lines.lock().unwrap().first().cloned() {
                break line;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        let asked = url::Url::parse(link.trim_start_matches("open ")).expect("the link parses");
        let query: std::collections::BTreeMap<_, _> = asked.query_pairs().into_owned().collect();
        assert_eq!(
            query.get("code_challenge_method").map(String::as_str),
            Some("S256")
        );
        assert_eq!(query.get("response_type").map(String::as_str), Some("code"));
        let back = query.get("redirect_uri").expect("a redirect was offered");
        let state = query.get("state").expect("a state was offered");
        let mut stream = tokio::net::TcpStream::connect(
            url::Url::parse(back)
                .expect("the redirect parses")
                .socket_addrs(|| None)
                .expect("the redirect resolves")[0],
        )
        .await
        .expect("the browser reaches the catcher");
        let get =
            format!("GET /callback?code=xyz&state={state} HTTP/1.1\r\nhost: localhost\r\n\r\n");
        tokio::io::AsyncWriteExt::write_all(&mut stream, get.as_bytes())
            .await
            .expect("the browser sends the redirect");
        query
            .get("code_challenge")
            .cloned()
            .expect("a challenge was sent")
    });

    let heard = asked.clone();
    let mut prompt = Prompt::telling(move |line| heard.lock().unwrap().push(line.to_string()));
    auth.login(&http, &mut prompt)
        .await
        .expect("the PKCE login completes");

    let challenge = visitor.await.expect("the browser finishes");
    served.await.expect("the mock finishes");

    let form = seen.lock().unwrap().clone();
    assert!(form.contains("grant_type=authorization_code"), "got {form}");
    assert!(form.contains("code=xyz"), "got {form}");
    let verifier = form
        .split("code_verifier=")
        .nth(1)
        .and_then(|rest| rest.split('&').next())
        .expect("the exchange spends a verifier");
    let proof = base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        <sha2::Sha256 as sha2::Digest>::digest(verifier.as_bytes()),
    );
    assert_eq!(proof, challenge, "the verifier must match the challenge");
    assert!(auth.ready().expect("the store answers"));
    assert_eq!(
        auth.token(&http).await.expect("the token is live"),
        "at",
        "the stored access token is the one the endpoint issued"
    );
}

#[test]
fn a_token_that_expires_within_the_skew_is_not_fresh() {
    let soon = Issued {
        access: "a".into(),
        refresh: None,
        expires: Some(Instant::now() + Duration::from_secs(5)),
        identity: None,
        client: None,
    };
    assert!(
        !soon.fresh(),
        "a token expiring inside the skew must be refreshed, not used"
    );
}

#[tokio::test]
async fn a_rejected_token_without_expiry_is_refreshed_on_the_next_call() {
    let issuer = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("the mock token endpoint binds");
    let token_url = format!(
        "http://{}/token",
        issuer.local_addr().expect("the mock has an address")
    );
    let served = tokio::spawn(async move {
        let (mut stream, _) = issuer.accept().await.expect("the mock takes the refresh");
        let seen = whole(&mut stream).await;
        let body = r#"{"access_token":"new"}"#;
        let reply = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        tokio::io::AsyncWriteExt::write_all(&mut stream, reply.as_bytes())
            .await
            .expect("the mock answers");
        seen
    });
    let kept = store();
    kept.set("svc", r#"{"access_token":"old","refresh_token":"r"}"#)
        .unwrap();
    let spec = Spec {
        token_url,
        client_id: "cli".into(),
        refresh_token: Some("seed".into()),
        ..Spec::default()
    };
    let auth = Auth::new(Method::Login(Box::new(spec)), Scheme::Bearer, "svc", kept);
    let http = http(Duration::from_secs(5), Duration::from_secs(5)).expect("a client builds");
    assert_eq!(auth.token(&http).await.unwrap(), "old");
    auth.forget_live();
    assert_eq!(
        auth.token(&http).await.expect("the refresh lands"),
        "new",
        "a rejected grant must take the refresh path, not be reused"
    );
    let seen = served.await.expect("the mock finishes");
    assert!(seen.contains("grant_type=refresh_token"), "got {seen}");
    assert!(seen.contains("refresh_token=r"), "got {seen}");
}

#[test]
fn a_token_reply_without_an_access_token_names_no_secret() {
    let Err(error) = oauth::minted(&serde_json::json!({
        "refresh_token": "rt-secret",
        "id_token": "id-secret"
    })) else {
        panic!("a reply without an access token was accepted");
    };
    let error = error.to_string();
    assert!(!error.contains("secret"), "got {error}");
    assert!(error.contains("refresh_token"), "got {error}");
}
