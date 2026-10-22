use std::ffi::OsString;
use std::path::Path;
use std::sync::mpsc::SyncSender;
#[cfg(unix)]
use std::sync::{Mutex, OnceLock};

use zeroize::Zeroizing;

use crate::error::Result;

pub(crate) type Secret = Zeroizing<String>;

pub struct Request {
    pub prompt: String,
    reply: SyncSender<Option<Secret>>,
}

impl Request {
    pub(crate) fn answer(self, secret: Option<Secret>) {
        drop(self.reply.send(secret));
    }
}

#[cfg(test)]
pub(crate) fn request(prompt: &str) -> (Request, std::sync::mpsc::Receiver<Option<Secret>>) {
    let (reply, answer) = std::sync::mpsc::sync_channel(1);
    (
        Request {
            prompt: prompt.to_string(),
            reply,
        },
        answer,
    )
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
const SCRUB_CHUNK: usize = 64 * 1024;

#[cfg(unix)]
const PROMPT_BYTES: u64 = 1024;

#[cfg(unix)]
const ASKER_CHARS: usize = 256;

#[cfg(unix)]
const RECOGNIZED: &[&str] = &["sudo", "ssh", "git", "doas"];

#[cfg(unix)]
fn labeled(asker: &str, prompt: &str) -> String {
    let asker = asker.trim();
    let name = asker
        .split_whitespace()
        .next()
        .and_then(|word| Path::new(word).file_name())
        .map_or_else(
            || "unknown".to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
    if RECOGNIZED.contains(&name.as_str()) {
        format!("{prompt} (asked by `{asker}`)")
    } else {
        format!("unrecognized program {name} asks for a password (`{asker}`): {prompt}")
    }
}

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
    crate::durable::store::scratch(
        &helper,
        format!(
            "#!/bin/sh\nexec {} askpass \"$@\"\n",
            crate::shell::quote(&exe)
        ),
    )?;
    set_permissions(&helper, Permissions::from_mode(0o700))?;
    if let Some(sudo) = on_path("sudo") {
        let shim = dir.path().join("sudo");
        crate::durable::store::scratch(
            &shim,
            format!(
                "#!/bin/sh\nexec {} -A \"$@\"\n",
                crate::shell::quote(&sudo.display().to_string())
            ),
        )?;
        set_permissions(&shim, Permissions::from_mode(0o700))?;
    }
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

    let asker = peer::asker(&stream, ASKER_CHARS);
    let mut prompt = String::new();
    {
        let mut reader = BufReader::new(Read::by_ref(&mut stream).take(PROMPT_BYTES));
        if reader.read_line(&mut prompt).is_err() {
            return;
        }
    }
    let (reply, answer) = std::sync::mpsc::sync_channel(1);
    send(Request {
        prompt: labeled(&asker, prompt.trim()),
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
pub fn env(path: &str) -> Vec<(String, String)> {
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
            ("PATH".to_string(), shim_path(&broker.helper, path)),
        ]
    })
}

pub(crate) fn on_path(name: &str) -> Option<std::path::PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?).find_map(|dir| {
        let path = dir.join(name);
        if path.is_file() {
            return Some(path);
        }
        let exe = dir.join(format!("{name}.exe"));
        (cfg!(windows) && Path::new(name).extension().is_none() && exe.is_file()).then_some(exe)
    })
}

#[cfg(unix)]
fn shim_path(helper: &Path, path: &str) -> String {
    let dirs = helper
        .parent()
        .map(Path::to_path_buf)
        .into_iter()
        .chain(std::env::split_paths(path));
    std::env::join_paths(dirs).map_or_else(
        |_| path.to_string(),
        |path| path.to_string_lossy().into_owned(),
    )
}

#[cfg(not(unix))]
pub fn env(_path: &str) -> Vec<(String, String)> {
    Vec::new()
}

#[cfg(unix)]
pub(crate) fn scrub(text: &str) -> String {
    let Some(secrets) = BROKER.get().and_then(|broker| broker.secrets.lock().ok()) else {
        return text.to_string();
    };
    crate::text::redact(text, secrets.iter().map(|secret| secret.as_str()))
}

#[cfg(not(unix))]
pub(crate) fn scrub(text: &str) -> String {
    text.to_string()
}

#[cfg(unix)]
pub(crate) fn scrub_file(path: &Path) -> bool {
    let Some(secrets) = BROKER.get().and_then(|broker| broker.secrets.lock().ok()) else {
        return false;
    };
    let needles: Vec<&[u8]> = secrets
        .iter()
        .map(|secret| secret.as_bytes())
        .filter(|needle| !needle.is_empty())
        .collect();
    if needles.is_empty() {
        return false;
    }
    rewrite(path, &needles).unwrap_or(false)
}

#[cfg(not(unix))]
pub(crate) fn scrub_file(_path: &Path) -> bool {
    false
}

#[cfg(unix)]
fn scrub_window(
    path: &Path,
    buf: &[u8],
    limit: usize,
    offset: u64,
    needles: &[&[u8]],
    temp: &mut Option<tempfile::NamedTempFile>,
) -> std::io::Result<usize> {
    use std::io::Write;

    let mut pos = 0;
    while pos < limit {
        match earliest(&buf[pos..], needles) {
            Some((at, len)) if pos + at < limit => {
                if temp.is_none() {
                    *temp = Some(copied_prefix(path, offset + pos as u64)?);
                }
                if let Some(out) = temp.as_mut() {
                    out.write_all(&buf[pos..pos + at])?;
                    out.write_all(b"[secret]")?;
                }
                pos += at + len;
            }
            Some(_) | None => {
                if let Some(out) = temp.as_mut() {
                    out.write_all(&buf[pos..limit])?;
                }
                pos = limit;
            }
        }
    }
    Ok(pos)
}

#[cfg(unix)]
fn rewrite(path: &Path, needles: &[&[u8]]) -> std::io::Result<bool> {
    use std::io::Read;

    let keep = needles.iter().map(|needle| needle.len()).max().unwrap_or(1) - 1;
    let mut file = std::fs::File::open(path)?;
    let mut temp: Option<tempfile::NamedTempFile> = None;
    let mut buf = Vec::with_capacity(SCRUB_CHUNK + keep);
    let mut chunk = vec![0_u8; SCRUB_CHUNK];
    let mut offset: u64 = 0;
    loop {
        let read = file.read(&mut chunk)?;
        buf.extend_from_slice(&chunk[..read]);
        let limit = if read == 0 {
            buf.len()
        } else {
            buf.len().saturating_sub(keep)
        };
        let pos = scrub_window(path, &buf, limit, offset, needles, &mut temp)?;
        buf.drain(..pos);
        offset += pos as u64;
        if read == 0 {
            break;
        }
    }
    let Some(temp) = temp else {
        return Ok(false);
    };
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|error| error.error)?;
    Ok(true)
}

#[cfg(unix)]
fn copied_prefix(path: &Path, len: u64) -> std::io::Result<tempfile::NamedTempFile> {
    use std::io::Read;

    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    std::io::copy(&mut std::fs::File::open(path)?.take(len), &mut temp)?;
    Ok(temp)
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

#[cfg(unix)]
mod peer;

#[cfg(all(test, unix))]
#[path = "askpass_tests.rs"]
mod tests;
