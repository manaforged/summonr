use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use leyline::Session;
use rand::TryRngCore;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use url::Url;

use crate::{Error, Parts, Prompt, Result, Store};

use super::store::{keep, save, stored};
use super::token::{exchange, minted, post};

use crate::LOGIN_WAIT as WAIT;
const HEAD_CEILING: usize = 8192;

pub(super) async fn login(
    parts: &Parts<'_>,
    http: &Session,
    store: &dyn Store,
    prompt: &mut Prompt,
) -> Result<()> {
    let authorize =
        parts.spec.authorize_url.as_deref().ok_or_else(|| {
            Error::Config("an authorization_code login needs authorize_url".into())
        })?;
    let (listener, redirect) = listen(parts.spec.redirect_uri.as_deref()).await?;
    let issued = stored(parts, store)?.and_then(|grant| grant.client);
    let client = issued.as_deref().unwrap_or(&parts.spec.client_id);
    let verifier = random(32)?;
    let state = random(16)?;
    let asked = target(authorize, parts, client, &redirect, &verifier, &state)?;

    prompt.notice(&format!("open {asked}"));
    if let Err(error) = crate::open::external(asked.as_str()) {
        prompt.notice(&format!(
            "the browser did not open ({error}); open the link above"
        ));
    }

    let back = returned(listener.as_ref(), prompt, &state).await?;
    let carried = listener.is_none().then_some(state.as_str());
    let client = back.client.as_deref().unwrap_or(client).to_string();
    let mut grant = redeemed(
        parts, http, &client, &redirect, &verifier, &back.code, carried,
    )
    .await?;
    grant.client = Some(client).filter(|used| *used != parts.spec.client_id);
    save(parts, store, grant.clone())?;
    let chat = exchange(parts.spec, http, grant).await?;
    keep(parts, chat)
}

async fn listen(wanted: Option<&str>) -> Result<(Option<TcpListener>, String)> {
    if !loopback(wanted) {
        return Ok((None, wanted.unwrap_or_default().to_string()));
    }
    let listener = bind(wanted).await?;
    let redirect = redirect(wanted, &listener)?;
    Ok((Some(listener), redirect))
}

async fn returned(
    listener: Option<&TcpListener>,
    prompt: &mut Prompt,
    state: &str,
) -> Result<Callback> {
    let Some(listener) = listener else {
        return pasted(prompt, state).await;
    };
    prompt.notice("waiting for the browser to come back");
    tokio::time::timeout(WAIT, caught(listener, state))
        .await
        .map_err(|_| Error::Denied("the login timed out".to_string()))?
}

struct Callback {
    code: String,
    client: Option<String>,
}

fn target(
    authorize: &str,
    parts: &Parts<'_>,
    client: &str,
    redirect: &str,
    verifier: &str,
    state: &str,
) -> Result<Url> {
    let mut asked =
        Url::parse(authorize).map_err(|error| Error::Config(format!("authorize_url: {error}")))?;
    {
        let mut query = asked.query_pairs_mut();
        query.append_pair("response_type", "code");
        query.append_pair("client_id", client);
        query.append_pair("redirect_uri", redirect);
        query.append_pair("code_challenge", &challenge(verifier));
        query.append_pair("code_challenge_method", "S256");
        query.append_pair("state", state);
        query.append_pair("nonce", &random(16)?);
        if let Some(scope) = &parts.spec.scope {
            query.append_pair("scope", scope);
        }
        for (key, value) in &parts.spec.authorize_params {
            query.append_pair(key, value);
        }
    }
    Ok(asked)
}

async fn redeemed(
    parts: &Parts<'_>,
    http: &Session,
    client: &str,
    redirect: &str,
    verifier: &str,
    code: &str,
    state: Option<&str>,
) -> Result<crate::Issued> {
    let mut form = vec![
        ("grant_type".to_string(), "authorization_code".to_string()),
        ("code".to_string(), code.to_string()),
        ("redirect_uri".to_string(), redirect.to_string()),
        ("client_id".to_string(), client.to_string()),
        ("code_verifier".to_string(), verifier.to_string()),
    ];
    if let Some(state) = state {
        form.push(("state".to_string(), state.to_string()));
    }
    if let Some(secret) = &parts.spec.client_secret {
        form.push(("client_secret".to_string(), secret.clone()));
    }
    for (key, value) in &parts.spec.token_params {
        form.push((key.clone(), value.clone()));
    }
    let (status, body) = post(http, &parts.spec.token_url, &form, parts.spec.token_style).await?;
    if status >= 400 {
        let said = body
            .get("error_description")
            .or_else(|| body.get("error"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("no reason given");
        return Err(Error::Denied(format!(
            "the token exchange failed: HTTP {status} {said}"
        )));
    }
    minted(&body)
}

async fn pasted(prompt: &mut Prompt, state: &str) -> Result<Callback> {
    let typed = prompt
        .request("paste the code shown in the browser, then press enter")
        .await?;
    let (code, sent) = typed
        .split_once('#')
        .map_or((typed.as_str(), None), |(code, sent)| (code, Some(sent)));
    if let Some(sent) = sent
        && sent != state
    {
        return Err(Error::Denied(
            "the pasted code carried the wrong state; start the login again".to_string(),
        ));
    }
    Ok(Callback {
        code: code.to_string(),
        client: None,
    })
}

fn loopback(wanted: Option<&str>) -> bool {
    let Some(raw) = wanted else {
        return true;
    };
    Url::parse(raw)
        .ok()
        .and_then(|url| url.host_str().map(str::to_ascii_lowercase))
        .is_some_and(|host| matches!(host.as_str(), "127.0.0.1" | "localhost" | "::1" | "[::1]"))
}

async fn bind(wanted: Option<&str>) -> Result<TcpListener> {
    let port = match wanted {
        Some(raw) => Url::parse(raw)
            .map_err(|error| Error::Config(format!("redirect_uri: {error}")))?
            .port()
            .ok_or_else(|| Error::Config("redirect_uri needs an explicit port".to_string()))?,
        None => 0,
    };
    TcpListener::bind(("127.0.0.1", port))
        .await
        .map_err(|error| Error::Transport(format!("cannot listen on 127.0.0.1:{port}: {error}")))
}

fn redirect(wanted: Option<&str>, listener: &TcpListener) -> Result<String> {
    let port = listener
        .local_addr()
        .map_err(|error| Error::Transport(error.to_string()))?
        .port();
    let Some(raw) = wanted else {
        return Ok(format!("http://127.0.0.1:{port}/callback"));
    };
    let mut asked =
        Url::parse(raw).map_err(|error| Error::Config(format!("redirect_uri: {error}")))?;
    if asked.port() != Some(0) {
        return Ok(raw.to_string());
    }
    asked
        .set_port(Some(port))
        .map_err(|()| Error::Config("redirect_uri cannot carry a port".to_string()))?;
    Ok(asked.to_string())
}

async fn caught(listener: &TcpListener, state: &str) -> Result<Callback> {
    loop {
        let (mut stream, _) = listener
            .accept()
            .await
            .map_err(|error| Error::Transport(error.to_string()))?;
        let Some(asked) = head(&mut stream).await? else {
            answer(&mut stream, "Waiting for the sign-in redirect.").await;
            continue;
        };
        let mut code = None;
        let mut sent = None;
        let mut refused = None;
        let mut client = None;
        for (key, value) in asked.query_pairs() {
            match key.as_ref() {
                "code" => code = Some(value.into_owned()),
                "state" => sent = Some(value.into_owned()),
                "error" => refused = Some(value.into_owned()),
                "client_id" => client = Some(value.into_owned()),
                _other => {}
            }
        }
        if let Some(error) = refused {
            answer(&mut stream, "Sign-in refused. Return to the terminal.").await;
            return Err(Error::Denied(format!("the login was refused: {error}")));
        }
        let (Some(code), Some(sent)) = (code, sent) else {
            answer(&mut stream, "Waiting for the sign-in redirect.").await;
            continue;
        };
        if sent != state {
            answer(&mut stream, "Sign-in rejected. Return to the terminal.").await;
            return Err(Error::Denied(
                "the redirect carried the wrong state; start the login again".to_string(),
            ));
        }
        answer(&mut stream, "Signed in. You can close this tab.").await;
        return Ok(Callback { code, client });
    }
}

async fn head(stream: &mut TcpStream) -> Result<Option<Url>> {
    let mut raw = vec![0u8; HEAD_CEILING];
    let read = stream
        .read(&mut raw)
        .await
        .map_err(|error| Error::Transport(error.to_string()))?;
    let text = String::from_utf8_lossy(&raw[..read]);
    let Some(path) = text.split_whitespace().nth(1) else {
        return Ok(None);
    };
    Ok(Url::parse(&format!("http://127.0.0.1{path}")).ok())
}

async fn answer(stream: &mut TcpStream, line: &str) {
    let body = format!(
        "<!doctype html><title>summonr</title><p style=\"font:16px system-ui;padding:3rem\">{line}"
    );
    let head = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        body.len()
    );
    drop(stream.write_all(head.as_bytes()).await);
    drop(stream.write_all(body.as_bytes()).await);
    drop(stream.flush().await);
}

pub(super) fn random(bytes: usize) -> Result<String> {
    let mut raw = vec![0u8; bytes];
    rand::rngs::OsRng
        .try_fill_bytes(&mut raw)
        .map_err(|error| Error::Config(format!("no system randomness: {error}")))?;
    Ok(URL_SAFE_NO_PAD.encode(raw))
}

pub(super) fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}
