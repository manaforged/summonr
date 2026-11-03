const START_SLACK_SECS: u64 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Owner {
    Job,
    Other,
    Gone,
}

pub(crate) fn alive_since(pid: u32, started: u64) -> bool {
    owner(pid, started) == Owner::Job
}

pub(crate) fn owner(pid: u32, started: u64) -> Owner {
    match crate::process::started(pid) {
        Some(created) if created <= started.saturating_add(START_SLACK_SECS) => Owner::Job,
        Some(_) => Owner::Other,
        None => probe(pid),
    }
}

#[cfg(not(unix))]
fn probe(_pid: u32) -> Owner {
    Owner::Gone
}

#[cfg(unix)]
#[expect(unsafe_code, reason = "std has no signal-zero liveness probe")]
fn probe(pid: u32) -> Owner {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return Owner::Gone;
    };
    // SAFETY: kill with signal 0 reads two integers, sends nothing, and touches no memory.
    let sent = unsafe { libc::kill(pid, 0) } == 0;
    if sent || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM) {
        Owner::Other
    } else {
        Owner::Gone
    }
}
