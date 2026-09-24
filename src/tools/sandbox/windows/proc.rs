use super::*;

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
        let name = wide("Local\\Summonr.Sandbox.Acl")?;
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
    let name = wide(format!("Local\\Summonr.Sandbox.{pid}"))?;
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

pub(crate) fn stop(pid: u32) -> io::Result<Option<OwnedHandle>> {
    // SAFETY: This opens a handle only for waiting on the named helper process.
    let raw = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    if raw.is_null() {
        let error = Error::last_os_error();
        return if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) {
            Ok(None)
        } else {
            Err(error)
        };
    }
    // SAFETY: OpenProcess returned a valid, exclusively owned handle.
    let process = unsafe { OwnedHandle::from_raw_handle(raw) };
    // SAFETY: The process guard remains live throughout the query.
    if unsafe { WaitForSingleObject(process.as_raw_handle(), 0) } == WAIT_OBJECT_0 {
        return Ok(None);
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
    Ok(Some(process))
}

pub(crate) fn wait(process: &OwnedHandle) -> io::Result<()> {
    // SAFETY: The caller owns the handle for the whole wait.
    match unsafe { WaitForSingleObject(process.as_raw_handle(), 30_000) } {
        WAIT_OBJECT_0 => Ok(()),
        WAIT_TIMEOUT => Err(Error::new(ErrorKind::TimedOut, "sandbox cleanup timed out")),
        _ => Err(Error::last_os_error()),
    }
}

fn stdio(kind: u32) -> io::Result<OwnedHandle> {
    let mut duplicate = null_mut();
    // SAFETY: The pseudo handle is borrowed; DuplicateHandle makes an owned copy of one stream, and only these copies enter the child's handle allowlist.
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
