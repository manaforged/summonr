use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::cron::{runner, store};
use crate::error::Error;

const ROUTE: &str = "/run/";
const BEARER: &str = "bearer";
const AUTHORIZATION: &str = "authorization";
const CONTENT_LENGTH: &str = "content-length";
const TRANSFER_ENCODING: &str = "transfer-encoding";
const MAX_HEADERS: usize = 32;
const CHUNK: usize = 4096;
const IN_FLIGHT: usize = 32;
const DRAIN_BYTES: usize = 1 << 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Status {
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

type Step<T> = std::result::Result<T, Status>;

impl Status {
    fn text(self) -> (&'static str, &'static str) {
        match self {
            Self::Accepted => ("202 Accepted", "accepted"),
            Self::BadRequest => ("400 Bad Request", "malformed request"),
            Self::Unauthorized => ("401 Unauthorized", "missing or wrong bearer token"),
            Self::NotFound => ("404 Not Found", "no such route; POST /run/NAME"),
            Self::WrongMethod => ("405 Method Not Allowed", "use POST"),
            Self::Timeout => ("408 Request Timeout", "request not received in time"),
            Self::Paused => ("409 Conflict", "job is paused"),
            Self::LengthRequired => (
                "411 Length Required",
                "send Content-Length, not chunked encoding",
            ),
            Self::TooLarge => ("413 Payload Too Large", "request too large"),
            Self::TooMany => ("429 Too Many Requests", "job fired too recently"),
            Self::Failed => ("500 Internal Server Error", "could not start the job"),
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
            body: serde_json::json!({ "error": status.text().1 }).to_string(),
            retry: None,
        }
    }
}

struct Conn<'a> {
    stream: &'a mut TcpStream,
    deadline: Instant,
}

impl Conn<'_> {
    fn read(&mut self, chunk: &mut [u8]) -> Step<usize> {
        let left = self.deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(Status::Timeout);
        }
        self.stream
            .set_read_timeout(Some(left))
            .map_err(|error| match error.kind() {
                ErrorKind::InvalidInput => Status::Timeout,
                _other => Status::Failed,
            })?;
        match self.stream.read(chunk) {
            Ok(0) => Err(Status::BadRequest),
            Ok(count) => Ok(count),
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                Err(Status::Timeout)
            }
            Err(_) => Err(Status::BadRequest),
        }
    }

    fn fill(&mut self, buffer: &mut Vec<u8>) -> Step<()> {
        let mut chunk = [0u8; CHUNK];
        let count = self.read(&mut chunk)?;
        buffer.extend_from_slice(&chunk[..count]);
        Ok(())
    }
}

pub(crate) fn serve(port: u16) -> crate::error::Result<i32> {
    let listener = Arc::new(TcpListener::bind((Ipv4Addr::LOCALHOST, port))?);
    let bound = listener.local_addr()?.port();
    println!("listening on http://127.0.0.1:{bound}");
    for _ in 0..IN_FLIGHT {
        let listener = Arc::clone(&listener);
        std::thread::Builder::new()
            .name("webhook".to_string())
            .spawn(move || {
                loop {
                    if let Ok((mut stream, _)) = listener.accept() {
                        handle(&mut stream);
                    }
                }
            })?;
    }
    let stopped = super::block(crate::interrupted());
    eprintln!("webhook listener on port {bound} stopped");
    stopped
}

pub(crate) fn decide(
    method: &str,
    path: &str,
    auth: Option<&str>,
    token: impl Fn(&str) -> Option<String>,
) -> Step<String> {
    let name = path
        .strip_prefix(ROUTE)
        .filter(|name| store::check_name(name).is_ok())
        .ok_or(Status::NotFound)?;
    if method != "POST" {
        return Err(Status::WrongMethod);
    }
    let presented = auth
        .and_then(|value| value.trim().split_once(' '))
        .filter(|(scheme, _)| scheme.eq_ignore_ascii_case(BEARER))
        .map(|(_, presented)| presented.trim());
    if presented
        .zip(token(name))
        .is_some_and(|(presented, stored)| store::same_secret(presented, &stored))
    {
        Ok(name.to_string())
    } else {
        Err(Status::Unauthorized)
    }
}

#[derive(Default)]
struct Trace {
    method: String,
    path: String,
    job: String,
}

struct Head {
    method: String,
    path: String,
    auth: Option<String>,
    length: usize,
    chunked: bool,
    offset: usize,
}

fn handle(stream: &mut TcpStream) {
    let start = Instant::now();
    let mut conn = Conn {
        stream,
        deadline: start + Duration::from_secs(crate::limits::limits().gateway_head_secs),
    };
    let mut trace = Trace::default();
    let answer = accept(&mut conn, &mut trace, start).unwrap_or_else(Answer::from);
    let dash = |field: &str| {
        if field.is_empty() {
            "-".to_string()
        } else {
            field.to_string()
        }
    };
    let job = if trace.job.is_empty() {
        String::new()
    } else {
        format!(" (job {})", trace.job)
    };
    let (line, _) = answer.status.text();
    eprintln!(
        "{} {} -> {line}{job}",
        dash(&trace.method),
        dash(&trace.path)
    );
    drop(respond(conn.stream, &answer));
}

fn accept(conn: &mut Conn<'_>, trace: &mut Trace, start: Instant) -> Step<Answer> {
    let mut buffer = Vec::new();
    let head = head(conn, &mut buffer)?;
    let path = head.path.split('?').next().unwrap_or_default();
    trace.method = head.method.chars().filter(char::is_ascii_graphic).collect();
    trace.path = path.chars().filter(char::is_ascii_graphic).collect();
    conn.deadline = start + Duration::from_secs(crate::limits::limits().gateway_read_secs);
    let name = decide(&head.method, path, head.auth.as_deref(), store::token)?;
    if head.chunked {
        return Err(Status::LengthRequired);
    }
    let mut body = buffer.split_off(head.offset);
    if head.length > crate::limits::limits().gateway_body_bytes {
        drain(conn, head.length.saturating_sub(body.len()));
        return Err(Status::TooLarge);
    }
    while body.len() < head.length {
        conn.fill(&mut body)?;
    }
    body.truncate(head.length);
    let answer = fire(&name, &body);
    if answer.status == Status::Accepted {
        trace.job = name;
    }
    Ok(answer)
}

fn head(conn: &mut Conn<'_>, buffer: &mut Vec<u8>) -> Step<Head> {
    loop {
        let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
        let mut parsed = httparse::Request::new(&mut headers);
        match parsed.parse(buffer) {
            Ok(httparse::Status::Complete(offset)) => {
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
                return Ok(Head {
                    method: parsed.method.unwrap_or_default().to_string(),
                    path: parsed.path.unwrap_or_default().to_string(),
                    auth: header(AUTHORIZATION).map(str::to_string),
                    length,
                    chunked: header(TRANSFER_ENCODING).is_some(),
                    offset,
                });
            }
            Ok(httparse::Status::Partial) => {}
            Err(_) => return Err(Status::BadRequest),
        }
        if buffer.len() > crate::limits::limits().gateway_body_bytes {
            return Err(Status::TooLarge);
        }
        conn.fill(buffer)?;
    }
}

fn fire(name: &str, body: &[u8]) -> Answer {
    let job = match store::get(name) {
        Ok(job) if job.paused => return Status::Paused.into(),
        Ok(job) => job,
        Err(Error::Setup(_)) => return Status::NotFound.into(),
        Err(_) => return Status::Failed.into(),
    };
    match runner::fire(name, (!body.is_empty()).then_some(body)) {
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
            retry: Some(runner::cooldown(&job, crate::cron::now()).unwrap_or(1)),
            ..Status::TooMany.into()
        },
        Err(Error::Config(_) | Error::Setup(_)) => Status::NotFound.into(),
        Err(_) => Status::Failed.into(),
    }
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
        answer.status.text().0,
        answer.body.len(),
        answer.body
    )?;
    stream.flush()
}
