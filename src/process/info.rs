#[cfg(unix)]
pub(crate) fn table() -> Vec<(u32, u32, String)> {
    let Ok(output) = std::process::Command::new("ps")
        .args(["-axo", "pid=,ppid=,command="])
        .output()
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let (pid, rest) = line.trim_start().split_once(char::is_whitespace)?;
            let rest = rest.trim_start();
            let (parent, command) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            Some((
                pid.parse().ok()?,
                parent.parse().ok()?,
                command.trim().to_string(),
            ))
        })
        .collect()
}

#[cfg(not(unix))]
pub(crate) fn table() -> Vec<(u32, u32, String)> {
    Vec::new()
}

#[cfg(unix)]
fn ps(field: &str, pid: u32) -> Option<String> {
    std::process::Command::new("ps")
        .args(["-o", field, "-p"])
        .arg(pid.to_string())
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(unix)]
pub(crate) fn parent(pid: u32) -> Option<u32> {
    ps("ppid=", pid)?.parse().ok()
}

#[cfg(unix)]
pub(crate) fn command(pid: u32) -> Option<String> {
    ps("command=", pid)
}

#[cfg(target_os = "macos")]
#[expect(unsafe_code, reason = "std has no process info query")]
pub(crate) fn started(pid: u32) -> Option<u64> {
    let pid = libc::c_int::try_from(pid).ok()?;
    // SAFETY: proc_bsdinfo is plain integers and byte arrays; all-zero is a valid value.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = libc::c_int::try_from(std::mem::size_of::<libc::proc_bsdinfo>()).ok()?;
    // SAFETY: the buffer pointer and size describe one owned proc_bsdinfo for the call.
    let written = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            std::ptr::from_mut(&mut info).cast(),
            size,
        )
    };
    (written == size).then_some(info.pbi_start_tvsec)
}

#[cfg(target_os = "linux")]
#[expect(unsafe_code, reason = "std has no clock-tick query")]
pub(crate) fn started(pid: u32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let ticks = stat
        .rsplit_once(')')?
        .1
        .split_whitespace()
        .nth(19)?
        .parse::<u64>()
        .ok()?;
    let boot = std::fs::read_to_string("/proc/stat")
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix("btime "))?
        .trim()
        .parse::<u64>()
        .ok()?;
    // SAFETY: sysconf reads one integer and touches no memory.
    let hertz = u64::try_from(unsafe { libc::sysconf(libc::_SC_CLK_TCK) }).ok()?;
    (hertz > 0).then(|| boot + ticks / hertz)
}

#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "Windows process identity requires handle-based Win32 queries"
)]
pub(crate) fn started(pid: u32) -> Option<u64> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};

    use windows_sys::Win32::Foundation::{FILETIME, WAIT_TIMEOUT};
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
        WaitForSingleObject,
    };

    // SAFETY: OpenProcess accepts a numeric PID and returns a separately owned handle.
    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            0,
            pid,
        )
    };
    if handle.is_null() {
        return None;
    }
    // SAFETY: OpenProcess returned a valid, exclusively owned handle.
    let process = unsafe { OwnedHandle::from_raw_handle(handle) };
    // SAFETY: The guard keeps the process handle valid throughout this query.
    if unsafe { WaitForSingleObject(process.as_raw_handle(), 0) } != WAIT_TIMEOUT {
        return None;
    }
    let mut created = FILETIME::default();
    let mut exited = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: All four output pointers are distinct, initialized FILETIME values.
    if unsafe {
        GetProcessTimes(
            process.as_raw_handle(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )
    } == 0
    {
        return None;
    }
    let ticks = (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime);
    (ticks / 10_000_000).checked_sub(11_644_473_600)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
pub(crate) fn started(_pid: u32) -> Option<u64> {
    None
}

#[cfg(target_os = "macos")]
#[expect(unsafe_code, reason = "proc_pid_rusage has no std wrapper")]
pub(crate) fn footprint_kb(pid: Option<u32>) -> Option<u64> {
    let target = match pid {
        Some(pid) => i32::try_from(pid).ok()?,
        None => i32::try_from(std::process::id()).ok()?,
    };
    let mut info = std::mem::MaybeUninit::<libc::rusage_info_v4>::zeroed();
    // SAFETY: proc_pid_rusage writes one rusage_info_v4 into the buffer; zero means success.
    let info = unsafe {
        (libc::proc_pid_rusage(target, libc::RUSAGE_INFO_V4, info.as_mut_ptr().cast()) == 0)
            .then(|| info.assume_init())
    }?;
    Some(
        match pid {
            Some(_) => info.ri_phys_footprint,
            None => info.ri_lifetime_max_phys_footprint,
        } / 1024,
    )
}

#[cfg(all(unix, not(target_os = "macos")))]
#[expect(unsafe_code, reason = "getrusage has no std wrapper")]
pub(crate) fn footprint_kb(pid: Option<u32>) -> Option<u64> {
    if let Some(pid) = pid {
        let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
        return status
            .lines()
            .find_map(|line| line.strip_prefix("VmRSS:"))?
            .trim()
            .trim_end_matches("kB")
            .trim()
            .parse()
            .ok();
    }
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::zeroed();
    // SAFETY: getrusage writes one rusage into the zeroed buffer; zero means success.
    let usage = unsafe {
        (libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) == 0).then(|| usage.assume_init())
    }?;
    u64::try_from(usage.ru_maxrss).ok()
}

#[cfg(not(unix))]
pub(crate) fn footprint_kb(_pid: Option<u32>) -> Option<u64> {
    None
}
