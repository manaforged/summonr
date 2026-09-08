use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};

use windows_sys::Win32::Foundation::{FILETIME, WAIT_TIMEOUT};
use windows_sys::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    WaitForSingleObject,
};

#[expect(
    unsafe_code,
    reason = "Windows process identity requires handle-based Win32 queries"
)]
pub(super) fn started(pid: u32) -> Option<u64> {
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
