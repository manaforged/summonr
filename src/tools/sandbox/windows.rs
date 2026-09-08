#![expect(
    unsafe_code,
    reason = "AppContainer, ACL, desktop, and process attributes have no safe standard-library API"
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
    DUPLICATE_SAME_ACCESS, DuplicateHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, LocalFree,
    WAIT_ABANDONED, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
    EXPLICIT_ACCESS_W, GRANT_ACCESS, GetNamedSecurityInfoW, GetSecurityInfo, REVOKE_ACCESS,
    SE_FILE_OBJECT, SE_WINDOW_OBJECT, SetEntriesInAclW, SetNamedSecurityInfoW, SetSecurityInfo,
    TRUSTEE_IS_SID, TRUSTEE_W,
};
use windows_sys::Win32::Security::Isolation::{
    CreateAppContainerProfile, DeleteAppContainerProfile,
};
use windows_sys::Win32::Security::{
    CONTAINER_INHERIT_ACE, CreateWellKnownSid, DACL_SECURITY_INFORMATION, FreeSid,
    OBJECT_INHERIT_ACE, PSID, SECURITY_ATTRIBUTES, SECURITY_CAPABILITIES, SID_AND_ATTRIBUTES,
    WinCapabilityInternetClientSid, WinCapabilityPrivateNetworkClientServerSid,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_GENERIC_EXECUTE, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
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
    CloseDesktop, CloseWindowStation, CreateDesktopW, CreateWindowStationW,
    GetProcessWindowStation, GetUserObjectInformationW, HDESK, HWINSTA, SetProcessWindowStation,
    UOI_NAME,
};
use windows_sys::Win32::System::Threading::{
    CREATE_UNICODE_ENVIRONMENT, CreateMutexW, CreateProcessW, DeleteProcThreadAttributeList,
    EXTENDED_STARTUPINFO_PRESENT, GetCurrentProcess, GetExitCodeProcess,
    InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST, OpenProcess,
    PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROC_THREAD_ATTRIBUTE_JOB_LIST,
    PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES, PROCESS_INFORMATION, PROCESS_SYNCHRONIZE,
    ReleaseMutex, STARTF_USESTDHANDLES, STARTUPINFOEXW, UpdateProcThreadAttribute,
    WaitForMultipleObjects, WaitForSingleObject,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    WINSTA_ACCESSGLOBALATOMS, WINSTA_CREATEDESKTOP, WINSTA_ENUMDESKTOPS, WINSTA_ENUMERATE,
    WINSTA_READATTRIBUTES,
};

const READ: u32 = FILE_GENERIC_READ | FILE_GENERIC_EXECUTE;
const MODIFY: u32 = READ | FILE_GENERIC_WRITE | 0x0001_0000;

#[cfg(test)]
#[path = "windows_tests.rs"]
mod tests;

struct Local(*mut c_void);

impl Drop for Local {
    fn drop(&mut self) {
        // SAFETY: This guard owns a LocalAlloc result returned by a Win32 security API.
        unsafe { LocalFree(self.0) };
    }
}

struct Profile {
    name: Vec<u16>,
    sid: PSID,
    grants: Vec<PathBuf>,
}

impl Profile {
    fn create(name: &str) -> io::Result<Self> {
        let name = wide(name)?;
        let mut sid = null_mut();
        // SAFETY: The name buffers are terminated, and sid is a valid output pointer.
        let result = unsafe {
            CreateAppContainerProfile(
                name.as_ptr(),
                name.as_ptr(),
                name.as_ptr(),
                null(),
                0,
                &mut sid,
            )
        };
        if result < 0 {
            return Err(Error::other(format!(
                "CreateAppContainerProfile failed: {result:#x}"
            )));
        }
        Ok(Self {
            name,
            sid,
            grants: Vec::new(),
        })
    }

    fn grant(&mut self, path: &Path, mask: u32) -> io::Result<()> {
        if self.grants.iter().any(|granted| granted == path) {
            return Ok(());
        }
        acl(path, self.sid, mask, false).map_err(|error| {
            Error::new(
                error.kind(),
                format!("cannot grant sandbox access to {}: {error}", path.display()),
            )
        })?;
        self.grants.push(path.to_path_buf());
        Ok(())
    }
}

impl Drop for Profile {
    fn drop(&mut self) {
        for path in self.grants.iter().rev() {
            if let Err(error) = acl(path, self.sid, 0, true) {
                eprintln!(
                    "sandbox: cannot remove temporary access to {}: {error}",
                    path.display()
                );
            }
        }
        // SAFETY: This guard owns the profile and SID returned by CreateAppContainerProfile.
        unsafe {
            let result = DeleteAppContainerProfile(self.name.as_ptr());
            if result < 0 {
                eprintln!("sandbox: cannot delete AppContainer profile: {result:#x}");
            }
            FreeSid(self.sid);
        }
    }
}

fn acl(path: &Path, sid: PSID, mask: u32, revoke: bool) -> io::Result<()> {
    let _lock = Lock::acquire()?;
    let path = wide(path)?;
    let mut descriptor = null_mut();
    let mut old = null_mut();
    // SAFETY: The terminated path and all output pointers remain valid for the call.
    status(unsafe {
        GetNamedSecurityInfoW(
            path.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            &mut old,
            null_mut(),
            &mut descriptor,
        )
    })?;
    let _descriptor = Local(descriptor);
    let entry = EXPLICIT_ACCESS_W {
        grfAccessPermissions: mask,
        grfAccessMode: if revoke { REVOKE_ACCESS } else { GRANT_ACCESS },
        grfInheritance: OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE,
        Trustee: TRUSTEE_W {
            TrusteeForm: TRUSTEE_IS_SID,
            ptstrName: sid.cast(),
            ..Default::default()
        },
    };
    let mut updated = null_mut();
    // SAFETY: The profile owns sid, descriptor owns old, and entry lives through this call.
    status(unsafe { SetEntriesInAclW(1, &entry, old, &mut updated) })?;
    let updated = Local(updated.cast());
    // SAFETY: updated owns the DACL. Propagation is limited to the explicitly granted directory.
    status(unsafe {
        SetNamedSecurityInfoW(
            path.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            updated.0.cast(),
            null_mut(),
        )
    })
}

struct Desktop {
    station: HWINSTA,
    desktop: HDESK,
    name: Vec<u16>,
    sid: PSID,
}

impl Desktop {
    fn create(profile: &Profile, name: &str) -> io::Result<Self> {
        let mut text = null_mut();
        // SAFETY: profile owns its SID, and text receives a LocalAlloc string.
        boolean(unsafe { ConvertSidToStringSidW(profile.sid, &mut text) })?;
        let text = Local(text.cast());
        let mut len = 0;
        // SAFETY: ConvertSidToStringSidW returns a terminated UTF-16 string.
        let sid = unsafe {
            let ptr = text.0.cast::<u16>();
            while *ptr.add(len) != 0 {
                len += 1;
            }
            String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len))
        };
        let sddl = wide(format!("D:(A;;GA;;;OW)(A;;GA;;;SY)(A;;GA;;;{sid})"))?;
        let mut descriptor = null_mut();
        // SAFETY: sddl is terminated and descriptor is a valid output pointer.
        boolean(unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut descriptor,
                null_mut(),
            )
        })?;
        let descriptor = Local(descriptor);
        let security = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: 0,
        };
        let desktop_name = wide(name)?;
        let mut desktop = Self {
            station: null_mut(),
            desktop: null_mut(),
            name: Vec::new(),
            sid: profile.sid,
        };
        // SAFETY: This runs only in the dedicated sandbox subprocess before it launches the tool.
        // Both UI objects have explicit ACLs and owned handles; the process station is restored.
        unsafe {
            desktop.station = CreateWindowStationW(
                null(),
                0,
                (WINSTA_READATTRIBUTES | WINSTA_CREATEDESKTOP) as u32 | 0x0006_0000,
                &security,
            );
            if desktop.station.is_null() {
                return Err(Error::other(format!(
                    "CreateWindowStationW: {}",
                    Error::last_os_error()
                )));
            }
            station_acl(desktop.station, profile.sid, false)?;
            let original = GetProcessWindowStation();
            boolean(SetProcessWindowStation(desktop.station))?;
            desktop.desktop = CreateDesktopW(
                desktop_name.as_ptr(),
                null(),
                null(),
                0,
                0x000f_01ff,
                &security,
            );
            let error = Error::last_os_error();
            boolean(SetProcessWindowStation(original))?;
            if desktop.desktop.is_null() {
                return Err(error);
            }
            let mut bytes = 0;
            GetUserObjectInformationW(desktop.station, UOI_NAME, null_mut(), 0, &mut bytes);
            let mut station = vec![0_u16; (bytes as usize).div_ceil(2)];
            boolean(GetUserObjectInformationW(
                desktop.station,
                UOI_NAME,
                station.as_mut_ptr().cast(),
                bytes,
                &mut bytes,
            ))?;
            let station = String::from_utf16_lossy(&station);
            desktop.name = wide(format!("{}\\{name}", station.trim_end_matches('\0')))?;
        }
        Ok(desktop)
    }
}

impl Drop for Desktop {
    fn drop(&mut self) {
        // SAFETY: These handles belong exclusively to this guard and use their matching closers.
        unsafe {
            if !self.desktop.is_null() {
                CloseDesktop(self.desktop);
            }
            if !self.station.is_null() {
                if let Err(error) = station_acl(self.station, self.sid, true) {
                    eprintln!("sandbox: cannot remove window station access: {error}");
                }
                CloseWindowStation(self.station);
            }
        }
    }
}

fn station_acl(station: HWINSTA, sid: PSID, revoke: bool) -> io::Result<()> {
    let _lock = Lock::acquire()?;
    let mut descriptor = null_mut();
    let mut old = null_mut();
    // SAFETY: The caller owns station and sid; output storage is valid through the call.
    status(unsafe {
        GetSecurityInfo(
            station,
            SE_WINDOW_OBJECT,
            DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            &mut old,
            null_mut(),
            &mut descriptor,
        )
    })?;
    let _descriptor = Local(descriptor);
    let entry = EXPLICIT_ACCESS_W {
        grfAccessPermissions: 0x0002_0000
            | (WINSTA_READATTRIBUTES
                | WINSTA_ACCESSGLOBALATOMS
                | WINSTA_ENUMDESKTOPS
                | WINSTA_ENUMERATE) as u32,
        grfAccessMode: if revoke { REVOKE_ACCESS } else { GRANT_ACCESS },
        grfInheritance: 0,
        Trustee: TRUSTEE_W {
            TrusteeForm: TRUSTEE_IS_SID,
            ptstrName: sid.cast(),
            ..Default::default()
        },
    };
    let mut updated = null_mut();
    // SAFETY: The original descriptor owns old, and sid outlives this update.
    status(unsafe { SetEntriesInAclW(1, &entry, old, &mut updated) })?;
    let updated = Local(updated.cast());
    // SAFETY: The handle and updated DACL remain valid for this call.
    status(unsafe {
        SetSecurityInfo(
            station,
            SE_WINDOW_OBJECT,
            DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            updated.0.cast(),
            null_mut(),
        )
    })
}

struct Attributes(Vec<usize>);

impl Attributes {
    fn new() -> io::Result<Self> {
        let mut size = 0;
        // SAFETY: The first call only obtains a size; the second receives aligned, owned storage.
        unsafe {
            InitializeProcThreadAttributeList(null_mut(), 3, 0, &mut size);
            if size == 0 {
                return Err(Error::last_os_error());
            }
            let mut storage = vec![0; size.div_ceil(size_of::<usize>())];
            boolean(InitializeProcThreadAttributeList(
                storage.as_mut_ptr().cast(),
                3,
                0,
                &mut size,
            ))?;
            Ok(Self(storage))
        }
    }

    fn pointer(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.0.as_mut_ptr().cast()
    }
}

impl Drop for Attributes {
    fn drop(&mut self) {
        // SAFETY: new initialized this list in storage owned by the guard.
        unsafe { DeleteProcThreadAttributeList(self.pointer()) };
    }
}

struct Lock(OwnedHandle);

impl Lock {
    fn acquire() -> io::Result<Self> {
        let name = wide("Local\\Summoner.Sandbox.Acl")?;
        // SAFETY: This mutex serializes ACL read-modify-write operations across helpers.
        let raw = unsafe { CreateMutexW(null(), 0, name.as_ptr()) };
        if raw.is_null() {
            return Err(Error::last_os_error());
        }
        // SAFETY: CreateMutexW returned an exclusively owned handle.
        let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
        // SAFETY: The guard owns the live mutex. Abandonment is safe because callers reread the ACL.
        match unsafe { WaitForSingleObject(handle.as_raw_handle(), 30_000) } {
            WAIT_OBJECT_0 | WAIT_ABANDONED => Ok(Self(handle)),
            WAIT_TIMEOUT => Err(Error::new(
                ErrorKind::TimedOut,
                "sandbox ACL update timed out",
            )),
            _ => Err(Error::last_os_error()),
        }
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        // SAFETY: acquire obtained this mutex on the current thread; no await occurs while held.
        unsafe { ReleaseMutex(self.0.as_raw_handle()) };
    }
}

fn job(pid: u32) -> io::Result<OwnedHandle> {
    let name = wide(format!("Local\\Summoner.Sandbox.{pid}"))?;
    // SAFETY: The terminated name identifies the helper's command job; this handle is owned.
    let raw = unsafe { CreateJobObjectW(null(), name.as_ptr()) };
    if raw.is_null() {
        return Err(Error::last_os_error());
    }
    // SAFETY: Read the creation status before any other Win32 call can replace it.
    let existed = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    // SAFETY: CreateJobObjectW returned a valid, exclusively owned handle.
    let job = unsafe { OwnedHandle::from_raw_handle(raw) };
    if existed {
        return Ok(job);
    }
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    // SAFETY: The job guard and correctly sized limits structure live through the call.
    boolean(unsafe {
        SetInformationJobObject(
            job.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    })?;
    Ok(job)
}

pub(crate) fn stop(pid: u32) -> io::Result<()> {
    // SAFETY: This opens a handle only for waiting on the named helper process.
    let raw = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    if raw.is_null() {
        let error = Error::last_os_error();
        return if error.raw_os_error() == Some(87) {
            Ok(())
        } else {
            Err(error)
        };
    }
    // SAFETY: OpenProcess returned a valid, exclusively owned handle.
    let process = unsafe { OwnedHandle::from_raw_handle(raw) };
    // SAFETY: The process guard remains live throughout the query.
    if unsafe { WaitForSingleObject(process.as_raw_handle(), 0) } == WAIT_OBJECT_0 {
        return Ok(());
    }
    let job = job(pid)?;
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags =
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
    // SAFETY: A zero process limit prevents a cancelled helper from launching a later command.
    boolean(unsafe {
        SetInformationJobObject(
            job.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    })?;
    // SAFETY: Termination affects the command job, leaving its helper alive to revoke ACLs.
    boolean(unsafe { TerminateJobObject(job.as_raw_handle(), 1) })?;
    // SAFETY: Both handles stay open until the helper has consumed cancellation and cleaned up.
    match unsafe { WaitForSingleObject(process.as_raw_handle(), 30_000) } {
        WAIT_OBJECT_0 => Ok(()),
        WAIT_TIMEOUT => Err(Error::new(ErrorKind::TimedOut, "sandbox cleanup timed out")),
        _ => Err(Error::last_os_error()),
    }
}

fn stdio(kind: u32) -> io::Result<OwnedHandle> {
    let mut duplicate = null_mut();
    // SAFETY: The current-process pseudo handle is borrowed; DuplicateHandle creates an owned copy
    // of one standard stream. Only these copies appear in the child's handle allowlist.
    unsafe {
        let process = GetCurrentProcess();
        boolean(DuplicateHandle(
            process,
            GetStdHandle(kind),
            process,
            &mut duplicate,
            0,
            1,
            DUPLICATE_SAME_ACCESS,
        ))?;
        Ok(OwnedHandle::from_raw_handle(duplicate))
    }
}

fn network(deny: bool) -> io::Result<Vec<Vec<u32>>> {
    if deny {
        return Ok(Vec::new());
    }
    [
        WinCapabilityInternetClientSid,
        WinCapabilityPrivateNetworkClientServerSid,
    ]
    .into_iter()
    .map(|kind| {
        let mut sid = vec![0_u32; 17];
        let mut len = 68;
        // SAFETY: A 68-byte aligned buffer holds SECURITY_MAX_SID_SIZE bytes.
        boolean(unsafe {
            CreateWellKnownSid(kind, null_mut(), sid.as_mut_ptr().cast(), &mut len)
        })?;
        Ok(sid)
    })
    .collect()
}

pub fn entry(args: &[OsString]) -> crate::Result<i32> {
    execute(args).map_err(|error| crate::Error::Tool(format!("Windows sandbox: {error}")))
}

fn execute(args: &[OsString]) -> io::Result<i32> {
    if args.len() < 6 {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "incomplete sandbox invocation",
        ));
    }
    let job = job(std::process::id())?;
    let parent = args[4]
        .to_str()
        .and_then(|value| value.parse::<u32>().ok())
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "invalid sandbox parent"))?;
    let parent = if std::env::var_os("SUMMONER_SANDBOX_DETACHED").is_some() {
        None
    } else {
        // SAFETY: The parent PID is supplied by the launcher; the returned handle is only waited on.
        let raw = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, parent) };
        if raw.is_null() {
            return Err(Error::last_os_error());
        }
        // SAFETY: OpenProcess returned an exclusively owned process handle.
        Some(unsafe { OwnedHandle::from_raw_handle(raw) })
    };
    let workspace = Path::new(&args[0]).canonicalize()?;
    let cwd = Path::new(&args[1]).canonicalize()?;
    let readonly = flag(&args[2])?;
    let deny_network = flag(&args[3])?;
    let executable = Path::new(&args[5]);
    let temp = tempfile::Builder::new()
        .prefix("summoner-sandbox-")
        .tempdir()?;
    let name = format!(
        "summoner.{}.{}",
        std::process::id(),
        temp.path()
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
    );
    let mut profile = Profile::create(&name)?;
    profile.grant(&workspace, if readonly { READ } else { MODIFY })?;
    profile.grant(temp.path(), MODIFY)?;
    let desktop = Desktop::create(&profile, &name)
        .map_err(|error| Error::other(format!("sandbox desktop: {error}")))?;
    let streams = [
        stdio(STD_INPUT_HANDLE)?,
        stdio(STD_OUTPUT_HANDLE)?,
        stdio(STD_ERROR_HANDLE)?,
    ];
    let handles: Vec<HANDLE> = streams.iter().map(AsRawHandle::as_raw_handle).collect();
    let mut network = network(deny_network)?;
    let mut capabilities: Vec<SID_AND_ATTRIBUTES> = network
        .iter_mut()
        .map(|sid| SID_AND_ATTRIBUTES {
            Sid: sid.as_mut_ptr().cast(),
            Attributes: 4,
        })
        .collect();
    let security = SECURITY_CAPABILITIES {
        AppContainerSid: profile.sid,
        Capabilities: if capabilities.is_empty() {
            null_mut()
        } else {
            capabilities.as_mut_ptr()
        },
        CapabilityCount: capabilities.len() as u32,
        Reserved: 0,
    };
    let mut attributes = Attributes::new()?;
    let jobs = [job.as_raw_handle()];
    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
    startup.StartupInfo.lpDesktop = desktop.name.as_ptr().cast_mut();
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdInput = handles[0];
    startup.StartupInfo.hStdOutput = handles[1];
    startup.StartupInfo.hStdError = handles[2];
    let mut process = PROCESS_INFORMATION::default();
    let exe = wide(executable)?;
    let environment = environment(temp.path(), &workspace, &cwd)?;
    let cwd = wide(&cwd)?;
    let mut command = command(executable.as_os_str(), &args[6..])?;
    // SAFETY: All referenced attribute values, strings, stream handles, and the job live until
    // CreateProcessW returns. JOB_LIST assigns containment atomically before any child code runs.
    unsafe {
        for (kind, pointer, size) in [
            (
                PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES,
                (&security as *const SECURITY_CAPABILITIES).cast(),
                size_of::<SECURITY_CAPABILITIES>(),
            ),
            (
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
                handles.as_ptr().cast(),
                handles.len() * size_of::<HANDLE>(),
            ),
            (
                PROC_THREAD_ATTRIBUTE_JOB_LIST,
                jobs.as_ptr().cast(),
                size_of::<HANDLE>(),
            ),
        ] {
            boolean(UpdateProcThreadAttribute(
                attributes.pointer(),
                0,
                kind as usize,
                pointer,
                size,
                null_mut(),
                null(),
            ))?;
        }
        startup.lpAttributeList = attributes.pointer();
        let created = CreateProcessW(
            exe.as_ptr(),
            command.as_mut_ptr(),
            null(),
            null(),
            1,
            EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT,
            environment.as_ptr().cast(),
            cwd.as_ptr(),
            &startup.StartupInfo,
            &mut process,
        );
        boolean(created).map_err(|error| Error::other(format!("CreateProcessW: {error}")))?;
    }
    // SAFETY: A successful CreateProcessW transfers these two distinct handles to the caller.
    let (process, thread) = unsafe {
        (
            OwnedHandle::from_raw_handle(process.hProcess),
            OwnedHandle::from_raw_handle(process.hThread),
        )
    };
    drop(thread);
    let mut code = 0;
    // SAFETY: The process guard owns a live handle, and code is a valid output pointer.
    unsafe {
        let mut handles = vec![process.as_raw_handle()];
        if let Some(parent) = &parent {
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
    drop(job);
    Ok(code as i32)
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
                "SUMMONER_SANDBOX_ROOT",
                "SUMMONER_SANDBOX_DIRECTORY",
            ]
            .iter()
            .any(|name| key.eq_ignore_ascii_case(name))
        })
        .collect();
    values.extend([
        (OsString::from("TEMP"), temp.as_os_str().to_owned()),
        (OsString::from("TMP"), temp.as_os_str().to_owned()),
        (
            OsString::from("SUMMONER_SANDBOX_ROOT"),
            simplified(workspace).as_os_str().to_owned(),
        ),
        (
            OsString::from("SUMMONER_SANDBOX_DIRECTORY"),
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
