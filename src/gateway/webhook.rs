use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::Arc;
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
const IN_FLIGHT: usize = 32;
const DRAIN_BYTES: usize = 1 << 20;
const POST: &str = "POST";

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
    WrongMethod,
    Timeout,
    Paused,
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
            Self::WrongMethod => "405 Method Not Allowed",
            Self::Timeout => "408 Request Timeout",
            Self::Paused => "409 Conflict",
            Self::LengthRequired => "411 Length Required",
            Self::TooLarge => "413 Payload Too Large",
            Self::TooMany => "429 Too Many Requests",
            Self::Failed => "500 Internal Server Error",
        }
    }

    fn reason(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::BadRequest => "malformed request",
            Self::Unauthorized => "missing or wrong bearer token",
            Self::NotFound => "no such route; POST /run/NAME",
            Self::WrongMethod => "use POST",
            Self::Timeout => "request not received in time",
            Self::Paused => "job is paused",
            Self::LengthRequired => "send Content-Length, not chunked encoding",
            Self::TooLarge => "request too large",
            Self::TooMany => "job fired too recently",
            Self::Failed => "could not start the job",
        }
    }
}

struct Answer {
    status: Status,
    body: String,
    retry: Option<u64>,
}

impl From<Status> for Answer {
    fn from(status: Status) -> Self {
        Self {
            status,
            body: serde_json::json!({ "error": status.reason() }).to_string(),
            retry: None,
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

#[derive(Default)]
struct Trace {
    method: String,
    path: String,
    job: Option<String>,
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
    let listener = Arc::new(TcpListener::bind((Ipv4Addr::LOCALHOST, port))?);
    let bound = listener.local_addr()?.port();
    println!("listening on http://127.0.0.1:{bound}");
    for _ in 0..IN_FLIGHT {
        let listener = Arc::clone(&listener);
        std::thread::Builder::new()
            .name("webhook".to_string())
            .spawn(move || work(&listener))?;
    }
    let stopped = super::block(crate::interrupted());
    eprintln!("webhook listener on port {bound} stopped");
    stopped
}

fn work(listener: &TcpListener) {
    loop {
        let Ok((mut stream, _)) = listener.accept() else {
            continue;
        };
        let answer = handle(&mut stream);
        drop(respond(&mut stream, &answer));
    }
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
    if method != POST || store::check_name(name).is_err() {
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

fn handle(stream: &mut TcpStream) -> Answer {
    let limits = crate::limits::limits();
    let start = Instant::now();
    let mut conn = Conn {
        stream,
        deadline: start + Duration::from_secs(limits.gateway_head_secs),
    };
    let mut trace = Trace::default();
    let answer = accept(&mut conn, &mut trace, start).unwrap_or_else(Answer::from);
    log(&trace, answer.status);
    answer
}

fn log(trace: &Trace, status: Status) {
    let method = if trace.method.is_empty() {
        "-"
    } else {
        &trace.method
    };
    let path = if trace.path.is_empty() {
        "-"
    } else {
        &trace.path
    };
    let job = trace
        .job
        .as_deref()
        .map_or_else(String::new, |job| format!(" (job {job})"));
    eprintln!("{method} {path} -> {}{job}", status.line());
}

fn accept(
    conn: &mut Conn<'_>,
    trace: &mut Trace,
    start: Instant,
) -> std::result::Result<Answer, Status> {
    let head = head(conn)?;
    let path = head.path.split('?').next().unwrap_or_default();
    trace.method = head.method.chars().filter(char::is_ascii_graphic).collect();
    trace.path = path.chars().filter(char::is_ascii_graphic).collect();
    conn.deadline = start + Duration::from_secs(crate::limits::limits().gateway_read_secs);
    let routed = path
        .strip_prefix(ROUTE)
        .is_some_and(|name| store::check_name(name).is_ok());
    if routed && head.method != POST {
        return Err(Status::WrongMethod);
    }
    let name = match decide(&head.method, path, head.auth.as_deref(), store::token) {
        Reply::Fire(name) => name,
        Reply::Unauthorized => return Err(Status::Unauthorized),
        Reply::NotFound => return Err(Status::NotFound),
    };
    if head.chunked {
        return Err(Status::LengthRequired);
    }
    let body = body(conn, head)?;
    let answer = fire(&name, &body);
    if answer.status == Status::Accepted {
        trace.job = Some(name);
    }
    Ok(answer)
}

fn fire(name: &str, body: &[u8]) -> Answer {
    let job = match store::get(name) {
        Ok(job) => job,
        Err(Error::Config(_)) => return Status::NotFound.into(),
        Err(_) => return Status::Failed.into(),
    };
    if job.paused {
        return Status::Paused.into();
    }
    let payload = (!body.is_empty()).then_some(body);
    match runner::fire(name, payload) {
        Ok(slot) => Answer {
            status: Status::Accepted,
            body: serde_json::json!({
                "job": name,
                "slot": slot,
                "log": format!("summonr cron log {name}"),
            })
            .to_string(),
            retry: None,
        },
        Err(Error::Denied(_)) => Answer {
            retry: Some(crate::cron::runner::cooldown(&job, crate::cron::now()).unwrap_or(1)),
            ..Status::TooMany.into()
        },
        Err(Error::Config(_)) => Status::NotFound.into(),
        Err(_) => Status::Failed.into(),
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
        drain(conn, head.length.saturating_sub(head.rest.len()));
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

fn drain(conn: &mut Conn<'_>, declared: usize) {
    let mut left = declared.min(DRAIN_BYTES);
    let mut chunk = [0u8; CHUNK];
    while left > 0 {
        let Ok(count) = conn.read(&mut chunk[..left.min(CHUNK)]) else {
            return;
        };
        left = left.saturating_sub(count);
    }
}

fn respond(stream: &mut TcpStream, answer: &Answer) -> std::io::Result<()> {
    let allow = if answer.status == Status::WrongMethod {
        "Allow: POST\r\n"
    } else {
        ""
    };
    let retry = answer
        .retry
        .map_or_else(String::new, |secs| format!("Retry-After: {secs}\r\n"));
    write!(
        stream,
        "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{allow}{retry}Connection: close\r\n\r\n{}",
        answer.status.line(),
        answer.body.len(),
        answer.body
    )?;
    stream.flush()
}
