use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use tokio::time::timeout;

use crate::{Error, Result};

const DEADLINE: Duration = Duration::from_secs(30);

/// A caller-owned offline backend task that returns captured request bodies.
pub(super) type Server = JoinHandle<Result<Vec<String>>>;

/// Serve two scripted replies and return the address and owned backend task.
pub(super) async fn server(replies: [String; 2]) -> Result<(String, Server)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}/v1", listener.local_addr()?);
    let task = tokio::spawn(async move {
        timeout(DEADLINE, async move {
            let mut bodies = Vec::new();
            for reply in replies {
                let (socket, _) = listener.accept().await?;
                let mut reader = BufReader::new(socket);
                bodies.push(read_body(&mut reader).await?);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}", reply.len()
                );
                reader.get_mut().write_all(response.as_bytes()).await?;
            }
            Ok(bodies)
        }).await.map_err(|error| Error::Tool(format!("offline backend deadline: {error}")))?
    });
    Ok((base, task))
}

async fn read_body(reader: &mut BufReader<TcpStream>) -> Result<String> {
    let mut length = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).await? == 0 {
            return Err(Error::Tool(
                "offline request ended before its headers".to_owned(),
            ));
        }
        if line == "\r\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            length = Some(value.trim().parse::<usize>().map_err(|error| {
                Error::Tool(format!("invalid offline request length: {error}"))
            })?);
        }
    }
    let length =
        length.ok_or_else(|| Error::Tool("offline request has no content length".to_owned()))?;
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes).await?;
    String::from_utf8(bytes)
        .map_err(|error| Error::Tool(format!("offline request is not UTF-8: {error}")))
}
