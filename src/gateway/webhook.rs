use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::time::{Duration, Instant};

use crate::cron::{runner, store};
use crate::error::{Error, Result};

const ROUTE: &str = "/run/";
const BEARER: &str = "bearer";
const AUTHORIZATION: &str = "authorization";
const CONTENT_LENGTH: &str = "content-length";
const TRANSFER_ENCODING: &str = "transfer-encoding";
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
    Timeout,
    LengthRequired,
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
            Self::Timeout => "408 Request Timeout",
            Self::LengthRequired => "411 Length Required",
            Self::TooLarge => "413 Payload Too Large",
            Self::TooMany => "429 Too Many Requests",
            Self::Failed => "500 Internal Server Error",
        }
    }
}

struct Head {
    method: String,
    path: String,
    auth: Option<String>,
    length: usize,
    chunked: bool,
    rest: Vec<u8>,
}

struct Conn<'a> {
    stream: &'a mut TcpStream,
    deadline: Instant,
}

impl Conn<'_> {
    fn read(&mut self, chunk: &mut [u8]) -> std::result::Result<usize, Status> {
        let left = self.deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(Status::Timeout);
        }
        self.stream
            .set_read_timeout(Some(left))
            .map_err(|error| match error.kind() {
                std::io::ErrorKind::InvalidInput => Status::Timeout,
                _other => Status::Failed,
            })?;
        match self.stream.read(chunk) {
            Ok(0) => Err(Status::BadRequest),
            Ok(count) => Ok(count),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                Err(Status::Timeout)
            }
            Err(_) => Err(Status::BadRequest),
        }
    }
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
    let presented = auth
        .and_then(|value| value.trim().split_once(' '))
        .filter(|(scheme, _)| scheme.eq_ignore_ascii_case(BEARER))
        .map(|(_, presented)| presented.trim());
    match (presented, token(name)) {
        (Some(presented), Some(stored)) if store::same_secret(presented, &stored) => {
            Reply::Fire(name.to_string())
        }
        (Some(_) | None, Some(_) | None) => Reply::Unauthorized,
    }
}

fn handle(stream: &mut TcpStream) -> (Status, String) {
    let mut conn = Conn {
        stream,
        deadline: Instant::now() + Duration::from_secs(crate::limits::limits().gateway_read_secs),
    };
    accept(&mut conn).unwrap_or_else(|status| (status, String::new()))
}

fn accept(conn: &mut Conn<'_>) -> std::result::Result<(Status, String), Status> {
    let head = head(conn)?;
    let path = head.path.split('?').next().unwrap_or_default();
    let name = match decide(&head.method, path, head.auth.as_deref(), store::token) {
        Reply::Fire(name) => name,
        Reply::Unauthorized => return Err(Status::Unauthorized),
        Reply::NotFound => return Err(Status::NotFound),
    };
    if head.chunked {
        return Err(Status::LengthRequired);
    }
    let body = body(conn, head)?;
    Ok(fire(&name, &body))
}

fn fire(name: &str, body: &[u8]) -> (Status, String) {
    let payload = (!body.is_empty()).then_some(body);
    match runner::fire(name, payload) {
        Ok(slot) => (
            Status::Accepted,
            serde_json::json!({ "job": name, "slot": slot }).to_string(),
        ),
        Err(Error::Denied(_)) => (Status::TooMany, String::new()),
        Err(Error::Config(_)) => (Status::NotFound, String::new()),
        Err(_) => (Status::Failed, String::new()),
    }
}

fn head(conn: &mut Conn<'_>) -> std::result::Result<Head, Status> {
    let cap = crate::limits::limits().gateway_body_bytes;
    let mut buffer = Vec::new();
    let mut chunk = [0u8; CHUNK];
    loop {
        let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
        let mut parsed = httparse::Request::new(&mut headers);
        match parsed.parse(&buffer) {
            Ok(httparse::Status::Complete(start)) => {
                return parts(&parsed, buffer[start..].to_vec());
            }
            Ok(httparse::Status::Partial) => {}
            Err(_) => return Err(Status::BadRequest),
        }
        if buffer.len() > cap {
            return Err(Status::TooLarge);
        }
        let count = conn.read(&mut chunk)?;
        buffer.extend_from_slice(&chunk[..count]);
    }
}

fn parts(parsed: &httparse::Request<'_, '_>, rest: Vec<u8>) -> std::result::Result<Head, Status> {
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
        Some(Err(_)) => return Err(Status::BadRequest),
    };
    Ok(Head {
        method: parsed.method.unwrap_or_default().to_string(),
        path: parsed.path.unwrap_or_default().to_string(),
        auth: header(AUTHORIZATION).map(str::to_string),
        length,
        chunked: header(TRANSFER_ENCODING).is_some(),
        rest,
    })
}

fn body(conn: &mut Conn<'_>, head: Head) -> std::result::Result<Vec<u8>, Status> {
    if head.length > crate::limits::limits().gateway_body_bytes {
        return Err(Status::TooLarge);
    }
    let mut body = head.rest;
    let mut chunk = [0u8; CHUNK];
    while body.len() < head.length {
        let count = conn.read(&mut chunk)?;
        body.extend_from_slice(&chunk[..count]);
    }
    body.truncate(head.length);
    Ok(body)
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
