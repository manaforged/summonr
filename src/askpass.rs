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
    let needles: Vec<&[u8]> = secrets
        .iter()
        .map(|secret| secret.as_bytes())
        .filter(|needle| !needle.is_empty())
        .collect();
    if needles.is_empty() {
        return;
    }
    drop(rewrite(path, &needles));
}

#[cfg(not(unix))]
pub fn scrub_file(_path: &Path) {}

#[cfg(unix)]
fn rewrite(path: &Path, needles: &[&[u8]]) -> std::io::Result<()> {
    use std::io::{Read, Write};

    const CHUNK: usize = 64 * 1024;
    let keep = needles.iter().map(|needle| needle.len()).max().unwrap_or(1) - 1;
    let mut file = std::fs::File::open(path)?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    let mut buf = Vec::with_capacity(CHUNK + keep);
    let mut chunk = vec![0_u8; CHUNK];
    let mut changed = false;
    loop {
        let read = file.read(&mut chunk)?;
        buf.extend_from_slice(&chunk[..read]);
        let limit = if read == 0 {
            buf.len()
        } else {
            buf.len().saturating_sub(keep)
        };
        let mut pos = 0;
        while pos < limit {
            match earliest(&buf[pos..], needles) {
                Some((at, len)) if pos + at < limit => {
                    temp.write_all(&buf[pos..pos + at])?;
                    temp.write_all(b"[secret]")?;
                    pos += at + len;
                    changed = true;
                }
                Some(_) | None => {
                    temp.write_all(&buf[pos..limit])?;
                    pos = limit;
                }
            }
        }
        buf.drain(..pos);
        if read == 0 {
            break;
        }
    }
    if changed {
        temp.as_file().sync_all()?;
        temp.persist(path).map_err(|error| error.error)?;
    }
    Ok(())
}

#[cfg(unix)]
fn earliest(haystack: &[u8], needles: &[&[u8]]) -> Option<(usize, usize)> {
    needles
        .iter()
        .filter_map(|needle| {
            haystack
                .windows(needle.len())
                .position(|window| window == *needle)
                .map(|at| (at, needle.len()))
        })
        .min()
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
