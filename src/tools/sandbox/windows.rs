#![expect(
    unsafe_code,
    reason = "Restricted tokens, ACLs, desktops, and process attributes have no safe standard-library API"
)]

use std::ffi::{OsStr, OsString, c_void};
use std::io::{self, Error, ErrorKind};
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};

use dunce::simplified;
use windows_sys::Win32::Foundation::{
    DUPLICATE_SAME_ACCESS, DuplicateHandle, ERROR_ALREADY_EXISTS, ERROR_INVALID_PARAMETER,
    GENERIC_ALL, GetLastError, HANDLE, LocalFree, WAIT_ABANDONED, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::Authorization::{
    ACCESS_MODE, ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
    DENY_ACCESS, EXPLICIT_ACCESS_W, GRANT_ACCESS, GetNamedSecurityInfoW, GetSecurityInfo,
    REVOKE_ACCESS, SE_FILE_OBJECT, SE_WINDOW_OBJECT, SetEntriesInAclW, SetNamedSecurityInfoW,
    SetSecurityInfo, TRUSTEE_IS_SID, TRUSTEE_W,
};
use windows_sys::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACL, ACL_SIZE_INFORMATION, AclSizeInformation, CONTAINER_INHERIT_ACE,
    CreateRestrictedToken, DACL_SECURITY_INFORMATION, DISABLE_MAX_PRIVILEGE, EqualSid, GetAce,
    GetAclInformation, GetLengthSid, GetTokenInformation, INHERITED_ACE, OBJECT_INHERIT_ACE, PSID,
    SECURITY_ATTRIBUTES, SID_AND_ATTRIBUTES, SetTokenInformation, TOKEN_ADJUST_DEFAULT,
    TOKEN_ASSIGN_PRIMARY, TOKEN_DEFAULT_DACL, TOKEN_DUPLICATE, TOKEN_GROUPS, TOKEN_QUERY,
    TokenDefaultDacl, TokenGroups, WRITE_RESTRICTED,
};
use windows_sys::Win32::Storage::FileSystem::{
    DELETE, FILE_APPEND_DATA, FILE_DELETE_CHILD, FILE_GENERIC_EXECUTE, FILE_GENERIC_READ,
    FILE_GENERIC_WRITE, FILE_WRITE_ATTRIBUTES, FILE_WRITE_DATA, FILE_WRITE_EA, READ_CONTROL,
    STANDARD_RIGHTS_REQUIRED, WRITE_DAC,
};
use windows_sys::Win32::System::Console::{
    GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
};
use windows_sys::Win32::System::JobObjects::{
    CreateJobObjectW, JOB_OBJECT_LIMIT_ACTIVE_PROCESS, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::StationsAndDesktops::{
    CloseDesktop, CloseWindowStation, CreateDesktopW, CreateWindowStationW, DESKTOP_CREATEMENU,
    DESKTOP_CREATEWINDOW, DESKTOP_ENUMERATE, DESKTOP_HOOKCONTROL, DESKTOP_JOURNALPLAYBACK,
    DESKTOP_JOURNALRECORD, DESKTOP_READOBJECTS, DESKTOP_SWITCHDESKTOP, DESKTOP_WRITEOBJECTS,
    GetProcessWindowStation, GetUserObjectInformationW, HDESK, HWINSTA, SetProcessWindowStation,
    UOI_NAME,
};
use windows_sys::Win32::System::SystemServices::{
    ACCESS_ALLOWED_ACE_TYPE, ACCESS_DENIED_ACE_TYPE, SE_GROUP_LOGON_ID,
};
use windows_sys::Win32::System::Threading::{
    CREATE_UNICODE_ENVIRONMENT, CreateMutexW, CreateProcessAsUserW, DeleteProcThreadAttributeList,
    EXTENDED_STARTUPINFO_PRESENT, GetCurrentProcess, GetExitCodeProcess,
    InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST, OpenProcess, OpenProcessToken,
    PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROC_THREAD_ATTRIBUTE_JOB_LIST, PROCESS_INFORMATION,
    PROCESS_SYNCHRONIZE, ReleaseMutex, STARTF_USESTDHANDLES, STARTUPINFOEXW,
    UpdateProcThreadAttribute, WaitForMultipleObjects, WaitForSingleObject,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    WINSTA_ACCESSGLOBALATOMS, WINSTA_CREATEDESKTOP, WINSTA_ENUMDESKTOPS, WINSTA_ENUMERATE,
    WINSTA_READATTRIBUTES,
};

mod acl;
mod launch;
mod proc;
mod token;

use acl::*;
use launch::*;
use proc::*;
pub(crate) use proc::{stop, wait};
use token::*;
pub(crate) const NETWORK_UNSUPPORTED: &str = "--no-net needs macOS Seatbelt or Linux bubblewrap; the Windows sandbox contains writes and keeps the network on";

pub fn entry(args: &[OsString]) -> crate::Result<i32> {
    execute(args).map_err(|error| crate::Error::Tool(format!("Windows sandbox: {error}")))
}

struct Invocation<'a> {
    workspace: PathBuf,
    cwd: PathBuf,
    readonly: bool,
    write: Vec<PathBuf>,
    executable: &'a Path,
    args: &'a [OsString],
}

impl<'a> Invocation<'a> {
    fn parse(args: &'a [OsString]) -> io::Result<Self> {
        if args.len() < 7 {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "incomplete sandbox invocation",
            ));
        }
        if flag(&args[3])? {
            return Err(Error::new(ErrorKind::Unsupported, NETWORK_UNSUPPORTED));
        }
        Ok(Self {
            workspace: Path::new(&args[0]).canonicalize()?,
            cwd: Path::new(&args[1]).canonicalize()?,
            readonly: flag(&args[2])?,
            write: write_paths(&args[4])?,
            executable: Path::new(&args[6]),
            args: &args[7..],
        })
    }
}

fn execute(args: &[OsString]) -> io::Result<i32> {
    let invocation = Invocation::parse(args)?;
    let job = job(std::process::id())?;
    let parent = parent_process(&args[5])?;
    let code = contain(&invocation, &job, parent.as_ref())?;
    drop(job);
    Ok(code as i32)
}

fn contain(
    run: &Invocation<'_>,
    job: &OwnedHandle,
    parent: Option<&OwnedHandle>,
) -> io::Result<u32> {
    let temp = tempfile::Builder::new()
        .prefix("summonr-sandbox-")
        .tempdir()?;
    let roots = grant_roots(&run.workspace, run.readonly, &run.write)?;
    let restricted = restricted(&roots)?;
    acl(temp.path(), restricted.logon.ptr(), MODIFY, GRANT_ACCESS)?;
    let desktop = Desktop::create(restricted.logon.ptr(), &sandbox_name(temp.path()))
        .map_err(|error| Error::other(format!("sandbox desktop: {error}")))?;
    let streams = std_streams()?;
    let handles: Vec<HANDLE> = streams.iter().map(AsRawHandle::as_raw_handle).collect();
    let process = launch(&Launch {
        token: &restricted.token,
        desktop: &desktop,
        handles: &handles,
        job,
        executable: run.executable,
        temp: temp.path(),
        workspace: &run.workspace,
        cwd: &run.cwd,
        args: run.args,
    })?;
    wait_exit(&process, parent, job)
}

fn parent_process(arg: &OsStr) -> io::Result<Option<OwnedHandle>> {
    let parent = arg
        .to_str()
        .and_then(|value| value.parse::<u32>().ok())
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "invalid sandbox parent"))?;
    if std::env::var_os("SUMMONR_SANDBOX_DETACHED").is_some() {
        return Ok(None);
    }
    // SAFETY: The parent PID is supplied by the launcher; the returned handle is only waited on.
    let raw = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, parent) };
    if raw.is_null() {
        return Err(Error::last_os_error());
    }
    // SAFETY: OpenProcess returned an exclusively owned process handle.
    Ok(Some(unsafe { OwnedHandle::from_raw_handle(raw) }))
}

fn write_paths(arg: &OsStr) -> io::Result<Vec<PathBuf>> {
    arg.to_string_lossy()
        .split(';')
        .filter(|path| !path.is_empty())
        .map(|path| Path::new(path).canonicalize())
        .collect()
}

fn sandbox_name(temp: &Path) -> String {
    format!(
        "summonr.{}.{}",
        std::process::id(),
        temp.file_name().unwrap_or_default().to_string_lossy()
    )
}

fn grant_roots(workspace: &Path, readonly: bool, write: &[PathBuf]) -> io::Result<Vec<Sid>> {
    let roots: Vec<&Path> = (!readonly)
        .then_some(workspace)
        .into_iter()
        .chain(write.iter().map(PathBuf::as_path))
        .collect();
    let mut sids = Vec::with_capacity(roots.len());
    for root in roots {
        let sid = Sid::for_path(root);
        grant(root, &sid, MODIFY, GRANT_ACCESS)?;
        sids.push(sid);
    }
    if !readonly {
        let sid = Sid::for_path(workspace);
        for path in crate::policy::guarded_git_paths(workspace) {
            grant(&path, &sid, GUARD, DENY_ACCESS)?;
        }
    }
    Ok(sids)
}

fn grant(path: &Path, sid: &Sid, mask: u32, mode: ACCESS_MODE) -> io::Result<()> {
    acl(path, sid.ptr(), mask, mode).map_err(|error| {
        Error::new(
            error.kind(),
            format!("cannot grant sandbox access to {}: {error}", path.display()),
        )
    })
}

fn std_streams() -> io::Result<[OwnedHandle; 3]> {
    Ok([
        stdio(STD_INPUT_HANDLE)?,
        stdio(STD_OUTPUT_HANDLE)?,
        stdio(STD_ERROR_HANDLE)?,
    ])
}

fn wait_exit(
    process: &OwnedHandle,
    parent: Option<&OwnedHandle>,
    job: &OwnedHandle,
) -> io::Result<u32> {
    let mut code = 0;
    // SAFETY: The process guard owns a live handle, and code is a valid output pointer.
    unsafe {
        let mut handles = vec![process.as_raw_handle()];
        if let Some(parent) = parent {
            handles.push(parent.as_raw_handle());
        }
        let result = WaitForMultipleObjects(handles.len() as u32, handles.as_ptr(), 0, u32::MAX);
        if result != WAIT_OBJECT_0 && result != WAIT_OBJECT_0 + 1 {
            return Err(Error::last_os_error());
        }
        boolean(TerminateJobObject(job.as_raw_handle(), 1))?;
        if WaitForSingleObject(process.as_raw_handle(), 30_000) != WAIT_OBJECT_0 {
            return Err(Error::new(
                ErrorKind::TimedOut,
                "sandbox command did not terminate",
            ));
        }
        boolean(GetExitCodeProcess(process.as_raw_handle(), &mut code))?;
    }
    Ok(code)
}

fn flag(value: &OsStr) -> io::Result<bool> {
    match value.to_str() {
        Some("true") => Ok(true),
        Some("false") => Ok(false),
        _ => Err(Error::new(ErrorKind::InvalidInput, "invalid sandbox flag")),
    }
}

fn environment(temp: &Path, workspace: &Path, cwd: &Path) -> io::Result<Vec<u16>> {
    let relative = cwd.strip_prefix(workspace).map_err(|error| {
        Error::new(
            ErrorKind::InvalidInput,
            format!("sandbox working directory is outside its workspace: {error}"),
        )
    })?;
    let mut values: Vec<(OsString, OsString)> = std::env::vars_os()
        .filter(|(key, _)| {
            ![
                "TEMP",
                "TMP",
                "SUMMONR_SANDBOX_ROOT",
                "SUMMONR_SANDBOX_DIRECTORY",
            ]
            .iter()
            .any(|name| key.eq_ignore_ascii_case(name))
        })
        .collect();
    values.extend([
        (OsString::from("TEMP"), temp.as_os_str().to_owned()),
        (OsString::from("TMP"), temp.as_os_str().to_owned()),
        (
            OsString::from("SUMMONR_SANDBOX_ROOT"),
            simplified(workspace).as_os_str().to_owned(),
        ),
        (
            OsString::from("SUMMONR_SANDBOX_DIRECTORY"),
            relative.as_os_str().to_owned(),
        ),
    ]);
    values.sort_by_cached_key(|(key, _)| key.to_string_lossy().to_ascii_lowercase());
    let mut block = Vec::new();
    for (mut key, value) in values {
        key.push("=");
        key.push(value);
        block.extend(wide(key)?);
    }
    block.push(0);
    Ok(block)
}

fn command(program: &OsStr, args: &[OsString]) -> io::Result<Vec<u16>> {
    let mut command = Vec::new();
    for arg in std::iter::once(program).chain(args.iter().map(OsString::as_os_str)) {
        if !command.is_empty() {
            command.push(b' ' as u16);
        }
        command.push(b'"' as u16);
        let mut slashes = 0;
        for unit in arg.encode_wide() {
            if unit == 0 {
                return Err(Error::new(
                    ErrorKind::InvalidInput,
                    "NUL in command argument",
                ));
            }
            if unit == b'\\' as u16 {
                slashes += 1;
                continue;
            }
            command.extend(std::iter::repeat_n(
                b'\\' as u16,
                if unit == b'"' as u16 {
                    slashes * 2 + 1
                } else {
                    slashes
                },
            ));
            slashes = 0;
            command.push(unit);
        }
        command.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2));
        command.push(b'"' as u16);
    }
    command.push(0);
    Ok(command)
}

fn wide(value: impl AsRef<OsStr>) -> io::Result<Vec<u16>> {
    let mut value: Vec<u16> = value.as_ref().encode_wide().collect();
    if value.contains(&0) {
        return Err(Error::new(ErrorKind::InvalidInput, "NUL in Windows string"));
    }
    value.push(0);
    Ok(value)
}

fn boolean(result: i32) -> io::Result<()> {
    if result == 0 {
        Err(Error::last_os_error())
    } else {
        Ok(())
    }
}

fn status(result: u32) -> io::Result<()> {
    if result == 0 {
        Ok(())
    } else {
        Err(Error::from_raw_os_error(result as i32))
    }
}
