use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::time::Duration;

use crate::cron::{runner, store};
use crate::error::{Error, Result};

const ROUTE: &str = "/run/";
const BEARER: &str = "Bearer ";
const AUTHORIZATION: &str = "authorization";
const CONTENT_LENGTH: &str = "content-length";
const MAX_HEADERS: usize = 32;
const CHUNK: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Reply {
    Fire(String),
    Unauthorized,
    NotFound,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    Accepted,
    BadRequest,
    Unauthorized,
    NotFound,
    TooLarge,
    TooMany,
    Failed,
}

impl Status {
    fn line(self) -> &'static str {
        match self {
            Self::Accepted => "202 Accepted",
            Self::BadRequest => "400 Bad Request",
            Self::Unauthorized => "401 Unauthorized",
            Self::NotFound => "404 Not Found",
            Self::TooLarge => "413 Payload Too Large",
            Self::TooMany => "429 Too Many Requests",
            Self::Failed => "500 Internal Server Error",
        }
    }
}

struct Request {
    method: String,
    path: String,
    auth: Option<String>,
    body: Vec<u8>,
}

enum Parsed {
    Done(Request),
    Fail(Status),
}

pub(crate) fn serve(port: u16) -> Result<i32> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))?;
    let bound = listener.local_addr()?.port();
    println!("listening on http://127.0.0.1:{bound}");
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        let (status, body) = handle(&mut stream);
        drop(respond(&mut stream, status, &body));
    }
    Ok(0)
}

pub(crate) fn decide(
    method: &str,
    path: &str,
    auth: Option<&str>,
    token: impl Fn(&str) -> Option<String>,
) -> Reply {
    let Some(name) = path.strip_prefix(ROUTE) else {
        return Reply::NotFound;
    };
    if method != "POST" || store::check_name(name).is_err() {
        return Reply::NotFound;
    }
    let presented = auth.and_then(|value| value.strip_prefix(BEARER));
    match (presented, token(name)) {
        (Some(presented), Some(stored)) if store::same_secret(presented, &stored) => {
            Reply::Fire(name.to_string())
        }
        (Some(_) | None, Some(_) | None) => Reply::Unauthorized,
    }
}

fn handle(stream: &mut TcpStream) -> (Status, String) {
    let request = match read(stream) {
        Parsed::Done(request) => request,
        Parsed::Fail(status) => return (status, String::new()),
    };
    let reply = decide(
        &request.method,
        &request.path,
        request.auth.as_deref(),
        store::token,
    );
    let name = match reply {
        Reply::Fire(name) => name,
        Reply::Unauthorized => return (Status::Unauthorized, String::new()),
        Reply::NotFound => return (Status::NotFound, String::new()),
    };
    let payload = (!request.body.is_empty()).then_some(request.body.as_slice());
    match runner::fire(&name, payload) {
        Ok(slot) => (
            Status::Accepted,
            serde_json::json!({ "job": name, "slot": slot }).to_string(),
        ),
        Err(Error::Denied(_)) => (Status::TooMany, String::new()),
        Err(Error::Config(_)) => (Status::NotFound, String::new()),
        Err(_) => (Status::Failed, String::new()),
    }
}

fn read(stream: &mut TcpStream) -> Parsed {
    let limits = crate::limits::limits();
    let cap = limits.gateway_body_bytes;
    if stream
        .set_read_timeout(Some(Duration::from_secs(
            crate::limits::limits().gateway_read_secs,
        )))
        .is_err()
    {
        return Parsed::Fail(Status::Failed);
    }
    let mut buffer = Vec::new();
    let mut chunk = [0u8; CHUNK];
    loop {
        let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
        let mut parsed = httparse::Request::new(&mut headers);
        match parsed.parse(&buffer) {
            Ok(httparse::Status::Complete(start)) => {
                return finish(stream, &parsed, buffer[start..].to_vec(), cap);
            }
            Ok(httparse::Status::Partial) => {}
            Err(_) => return Parsed::Fail(Status::BadRequest),
        }
        if buffer.len() > cap {
            return Parsed::Fail(Status::TooLarge);
        }
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return Parsed::Fail(Status::BadRequest),
            Ok(count) => buffer.extend_from_slice(&chunk[..count]),
        }
    }
}

fn finish(
    stream: &mut TcpStream,
    parsed: &httparse::Request<'_, '_>,
    mut body: Vec<u8>,
    cap: usize,
) -> Parsed {
    let header = |wanted: &str| {
        parsed
            .headers
            .iter()
            .find(|header| header.name.eq_ignore_ascii_case(wanted))
            .and_then(|header| std::str::from_utf8(header.value).ok())
            .map(str::trim)
    };
    let length = match header(CONTENT_LENGTH).map(str::parse::<usize>) {
        None => 0,
        Some(Ok(length)) => length,
        Some(Err(_)) => return Parsed::Fail(Status::BadRequest),
    };
    if length > cap {
        return Parsed::Fail(Status::TooLarge);
    }
    if body.len() < length {
        let mut rest = vec![0u8; length - body.len()];
        if stream.read_exact(&mut rest).is_err() {
            return Parsed::Fail(Status::BadRequest);
        }
        body.extend_from_slice(&rest);
    }
    body.truncate(length);
    Parsed::Done(Request {
        method: parsed.method.unwrap_or_default().to_string(),
        path: parsed.path.unwrap_or_default().to_string(),
        auth: header(AUTHORIZATION).map(str::to_string),
        body,
    })
}

fn respond(stream: &mut TcpStream, status: Status, body: &str) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        status.line(),
        body.len()
    )?;
    stream.flush()
}
