use std::ffi::OsString;
use std::path::Path;
use std::sync::mpsc::SyncSender;
#[cfg(unix)]
use std::sync::{Mutex, OnceLock};

use zeroize::Zeroizing;

use crate::error::Result;

pub type Secret = Zeroizing<String>;

pub struct Request {
    pub prompt: String,
    reply: SyncSender<Option<Secret>>,
}

impl Request {
    pub(crate) fn answer(self, secret: Option<Secret>) {
        drop(self.reply.send(secret));
    }
}

#[cfg(unix)]
struct Broker {
    _dir: tempfile::TempDir,
    socket: std::path::PathBuf,
    helper: std::path::PathBuf,
    secrets: Mutex<Vec<Secret>>,
}

#[cfg(unix)]
static BROKER: OnceLock<Broker> = OnceLock::new();

#[cfg(unix)]
const SHORTEST: usize = 4;

#[cfg(unix)]
pub fn start(send: impl Fn(Request) + Send + 'static) -> Result<()> {
    use std::fs::{Permissions, set_permissions};
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixListener;

    if BROKER.get().is_some() {
        return Ok(());
    }
    let dir = tempfile::Builder::new()
        .prefix("summonr-askpass-")
        .tempdir()?;
    set_permissions(dir.path(), Permissions::from_mode(0o700))?;
    let socket = dir.path().join("socket");
    let helper = dir.path().join("askpass");
    let exe = std::env::current_exe()?.display().to_string();
    crate::durable::store::write(
        &helper,
        format!(
            "#!/bin/sh\nexec '{}' askpass \"$@\"\n",
            exe.replace('\'', "'\\''")
        ),
    )?;
    set_permissions(&helper, Permissions::from_mode(0o700))?;
    let listener = UnixListener::bind(&socket)?;
    set_permissions(&socket, Permissions::from_mode(0o600))?;
    drop(BROKER.set(Broker {
        _dir: dir,
        socket,
        helper,
        secrets: Mutex::new(Vec::new()),
    }));
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            serve(stream, &send);
        }
    });
    Ok(())
}

#[cfg(not(unix))]
pub fn start(_send: impl Fn(Request) + Send + 'static) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn serve(mut stream: std::os::unix::net::UnixStream, send: &impl Fn(Request)) {
    use std::io::{BufRead, BufReader, Read, Write};

    let mut prompt = String::new();
    if BufReader::new(Read::by_ref(&mut stream).take(512))
        .read_line(&mut prompt)
        .is_err()
    {
        return;
    }
    let (reply, answer) = std::sync::mpsc::sync_channel(1);
    send(Request {
        prompt: prompt.trim().to_string(),
        reply,
    });
    let Ok(Some(secret)) = answer.recv() else {
        return;
    };
    if let Some(broker) = BROKER.get()
        && secret.len() >= SHORTEST
        && let Ok(mut secrets) = broker.secrets.lock()
        && !secrets.contains(&secret)
    {
        secrets.push(secret.clone());
    }
    drop(stream.write_all(secret.as_bytes()));
}

#[cfg(unix)]
pub fn env() -> Vec<(String, String)> {
    BROKER.get().map_or_else(Vec::new, |broker| {
        let helper = broker.helper.display().to_string();
        vec![
            ("SUDO_ASKPASS".to_string(), helper.clone()),
            ("SSH_ASKPASS".to_string(), helper),
            ("SSH_ASKPASS_REQUIRE".to_string(), "force".to_string()),
            (
                "SUMMONR_ASKPASS".to_string(),
                broker.socket.display().to_string(),
            ),
        ]
    })
}

#[cfg(not(unix))]
pub fn env() -> Vec<(String, String)> {
    Vec::new()
}

#[cfg(unix)]
pub fn scrub(text: &str) -> String {
    let Some(secrets) = BROKER.get().and_then(|broker| broker.secrets.lock().ok()) else {
        return text.to_string();
    };
    secrets.iter().fold(text.to_string(), |text, secret| {
        text.replace(secret.as_str(), "[secret]")
    })
}

#[cfg(not(unix))]
pub fn scrub(text: &str) -> String {
    text.to_string()
}

#[cfg(unix)]
pub fn scrub_file(path: &Path) {
    let Some(secrets) = BROKER.get().and_then(|broker| broker.secrets.lock().ok()) else {
        return;
    };
    if secrets.is_empty() {
        return;
    }
    let Ok(bytes) = std::fs::read(path) else {
        return;
    };
    let clean = secrets.iter().fold(bytes.clone(), |bytes, secret| {
        replace(&bytes, secret.as_bytes(), b"[secret]")
    });
    if clean != bytes {
        drop(crate::durable::store::write(path, clean));
    }
}

#[cfg(not(unix))]
pub fn scrub_file(_path: &Path) {}

#[cfg(unix)]
fn replace(haystack: &[u8], needle: &[u8], with: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(haystack.len());
    let mut rest = haystack;
    while let Some(at) = rest
        .windows(needle.len())
        .position(|window| window == needle)
    {
        out.extend_from_slice(&rest[..at]);
        out.extend_from_slice(with);
        rest = &rest[at + needle.len()..];
    }
    out.extend_from_slice(rest);
    out
}

pub fn client(args: &[OsString]) -> i32 {
    let prompt = args
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(" ");
    let Some(socket) = std::env::var_os("SUMMONR_ASKPASS") else {
        eprintln!("summonr askpass: a secret prompt needs an interactive summonr session");
        return 1;
    };
    ask(&socket, &prompt)
}

#[cfg(unix)]
fn ask(socket: &std::ffi::OsStr, prompt: &str) -> i32 {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;

    let Ok(mut stream) = UnixStream::connect(socket) else {
        eprintln!("summonr askpass: the summonr session is not reachable");
        return 1;
    };
    if writeln!(stream, "{}", prompt.replace(['\n', '\r'], " ")).is_err() {
        return 1;
    }
    let mut secret = Zeroizing::new(String::new());
    if stream.read_to_string(&mut secret).is_err() || secret.is_empty() {
        eprintln!("summonr askpass: cancelled");
        return 1;
    }
    let mut out = std::io::stdout().lock();
    if writeln!(out, "{}", secret.as_str()).is_err() || out.flush().is_err() {
        return 1;
    }
    0
}

#[cfg(not(unix))]
fn ask(_socket: &std::ffi::OsStr, _prompt: &str) -> i32 {
    eprintln!("summonr askpass: secret prompts need a Unix host");
    1
}
