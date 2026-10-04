use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;

pub(super) fn asker(stream: &UnixStream, limit: usize) -> String {
    peer_pid(stream)
        .and_then(parent_of)
        .map(command_of)
        .unwrap_or_default()
        .replace(['\n', '\r'], " ")
        .chars()
        .take(limit)
        .collect()
}

fn ps(field: &str, pid: u32) -> Option<String> {
    std::process::Command::new("ps")
        .args(["-o", field, "-p"])
        .arg(pid.to_string())
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn parent_of(pid: u32) -> Option<u32> {
    ps("ppid=", pid)?.parse().ok()
}

fn command_of(pid: u32) -> String {
    ps("command=", pid).unwrap_or_default()
}

#[cfg(target_os = "linux")]
#[expect(unsafe_code, reason = "std has no stable peer-credential query")]
fn peer_pid(stream: &UnixStream) -> Option<u32> {
    let mut cred = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut size = libc::socklen_t::try_from(std::mem::size_of::<libc::ucred>()).ok()?;
    // SAFETY: the fd is a live Unix socket owned by `stream`, and `cred` and `size` are valid for writes of the sizes passed.
    let status = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            std::ptr::from_mut(&mut cred).cast(),
            &mut size,
        )
    };
    (status == 0)
        .then_some(cred.pid)
        .and_then(|pid| u32::try_from(pid).ok())
}

#[cfg(target_os = "macos")]
#[expect(unsafe_code, reason = "std has no stable peer-credential query")]
fn peer_pid(stream: &UnixStream) -> Option<u32> {
    let mut pid: libc::pid_t = 0;
    let mut size = libc::socklen_t::try_from(std::mem::size_of::<libc::pid_t>()).ok()?;
    // SAFETY: the fd is a live Unix socket owned by `stream`, and `pid` and `size` are valid for writes of the sizes passed.
    let status = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_LOCAL,
            libc::LOCAL_PEERPID,
            std::ptr::from_mut(&mut pid).cast(),
            &mut size,
        )
    };
    (status == 0)
        .then_some(pid)
        .and_then(|pid| u32::try_from(pid).ok())
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn peer_pid(_stream: &UnixStream) -> Option<u32> {
    None
}
