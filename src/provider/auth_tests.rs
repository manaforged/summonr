use std::io::{Read, Write};
use std::net::TcpListener;

use super::*;

fn spec(token_url: String) -> OAuth {
    OAuth {
        token_url,
        client_id: "id".into(),
        client_secret: Some("secret".into()),
        refresh_token: None,
        scope: Some("chat".into()),
        grant: None,
        device_url: None,
        exchange_url: None,
    }
}

fn serve_seq(
    listener: TcpListener,
    bodies: Vec<(u16, &'static str)>,
) -> std::io::Result<Vec<String>> {
    let mut requests = Vec::new();
    for (status, body) in bodies {
        let (mut stream, _) = listener.accept()?;
        let mut request = Vec::new();
        let mut buffer = [0_u8; 1024];
        let header_end = loop {
            let count = stream.read(&mut buffer)?;
            if count == 0 {
                break request.len();
            }
            request.extend_from_slice(&buffer[..count]);
            if let Some(end) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                break end + 4;
            }
        };
        let headers = String::from_utf8_lossy(&request[..header_end.min(request.len())]);
        let length = headers
            .lines()
            .find_map(|line| {
                line.strip_prefix("Content-Length:")
                    .and_then(|value| value.trim().parse::<usize>().ok())
            })
            .unwrap_or(0);
        while request.len() < header_end + length {
            let count = stream.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..count]);
        }
        let reason = if status == 200 { "OK" } else { "ERR" };
        let response = format!(
            "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes())?;
        requests.push(String::from_utf8_lossy(&request).into_owned());
    }
    Ok(requests)
}

#[tokio::test]
async fn oauth_client_credentials_fetches_a_bearer() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let server = std::thread::spawn(move || {
        serve_seq(
            listener,
            vec![(
                200,
                r#"{"access_token":"tok-1","token_type":"Bearer","expires_in":3600}"#,
            )],
        )
    });
    let home = tempfile::tempdir()?;
    let auth = Auth::oauth_in(
        spec(format!("http://{address}/oauth/token")),
        "cc",
        home.path().to_path_buf(),
    )?;
    let http = reqwest::Client::new();
    let token = auth.token(&http).await?;
    assert_eq!(token, "tok-1");
    assert_eq!(auth.token(&http).await?, "tok-1");
    let posted = server
        .join()
        .map_err(|_panic| Error::Config("oauth fixture panicked".to_string()))??;
    assert!(posted[0].contains("grant_type=client_credentials"));
    assert!(posted[0].contains("client_id=id"));
    Ok(())
}

#[tokio::test]
async fn oauth_refresh_token_grant() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let server = std::thread::spawn(move || {
        serve_seq(
            listener,
            vec![(200, r#"{"access_token":"tok-r","expires_in":60}"#)],
        )
    });
    let home = tempfile::tempdir()?;
    let mut oauth = spec(format!("http://{address}/token"));
    oauth.client_secret = None;
    oauth.refresh_token = Some("r1".into());
    oauth.scope = None;
    let auth = Auth::oauth_in(oauth, "rt", home.path().to_path_buf())?;
    let http = reqwest::Client::new();
    assert_eq!(auth.token(&http).await?, "tok-r");
    let posted = server
        .join()
        .map_err(|_panic| Error::Config("oauth fixture panicked".to_string()))??;
    assert!(posted[0].contains("grant_type=refresh_token"));
    assert!(posted[0].contains("refresh_token=r1"));
    Ok(())
}

#[tokio::test]
async fn device_login_polls_until_a_token() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let base = format!("http://{address}");
    let server = std::thread::spawn(move || {
        serve_seq(
            listener,
            vec![
                (
                    200,
                    r#"{"device_code":"dc","user_code":"WD-42","verification_uri":"http://verify","interval":0,"expires_in":60}"#,
                ),
                (400, r#"{"error":"authorization_pending"}"#),
                (
                    200,
                    r#"{"access_token":"tok-d","refresh_token":"ref-d","expires_in":3600}"#,
                ),
            ],
        )
    });
    let home = tempfile::tempdir()?;
    let mut oauth = spec(format!("{base}/token"));
    oauth.device_url = Some(format!("{base}/device"));
    oauth.grant = Some("device_code".into());
    oauth.scope = None;
    let auth = Auth::oauth_in(oauth, "dev", home.path().to_path_buf())?;
    let http = reqwest::Client::new();
    let mut notes = Vec::new();
    auth.login(&http, |line| notes.push(line.to_string()))
        .await?;
    assert!(notes.iter().any(|line| line.contains("WD-42")));
    assert_eq!(auth.token(&http).await?, "tok-d");
    let posted = server
        .join()
        .map_err(|_panic| Error::Config("oauth fixture panicked".to_string()))??;
    assert!(posted[0].contains("/device") || posted[0].contains("client_id=id"));
    assert!(posted[2].contains("device_code=dc"));
    Ok(())
}

#[tokio::test]
async fn device_token_without_login_asks_for_login() -> Result<()> {
    let home = tempfile::tempdir()?;
    let mut oauth = spec("http://127.0.0.1:1/token".into());
    oauth.device_url = Some("http://127.0.0.1:1/device".into());
    let auth = Auth::oauth_in(oauth, "need", home.path().to_path_buf())?;
    let http = reqwest::Client::new();
    let error = auth.token(&http).await.unwrap_err();
    assert!(error.to_string().contains("summon login need"));
    Ok(())
}

#[test]
fn oauth_needs_token_url_and_client_id() {
    let home = tempfile::tempdir().expect("tmp");
    let error = Auth::oauth_in(
        OAuth {
            token_url: String::new(),
            client_id: "id".into(),
            client_secret: None,
            refresh_token: None,
            scope: None,
            grant: None,
            device_url: None,
            exchange_url: None,
        },
        "x",
        home.path().to_path_buf(),
    );
    assert!(error.is_err());
}

#[test]
fn a_huge_expiry_does_not_overflow_the_monotonic_clock() {
    let far = super::deadline(u64::MAX);
    assert!(far > std::time::Instant::now());
    let soon = super::deadline(60);
    assert!(soon > std::time::Instant::now());
}

#[test]
fn credential_paths_do_not_collide_across_punctuation() {
    let home = Path::new("/tmp/h");
    assert_eq!(
        credentials_path(home, "glm"),
        home.join("credentials/glm.json")
    );
    assert_ne!(
        credentials_path(home, "foo.bar"),
        credentials_path(home, "foo-bar")
    );
}
