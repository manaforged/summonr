use super::*;

pub(super) struct Launch<'a> {
    pub(super) token: &'a OwnedHandle,
    pub(super) desktop: &'a Desktop,
    pub(super) handles: &'a [HANDLE],
    pub(super) job: &'a OwnedHandle,
    pub(super) executable: &'a Path,
    pub(super) temp: &'a Path,
    pub(super) workspace: &'a Path,
    pub(super) cwd: &'a Path,
    pub(super) args: &'a [OsString],
}

pub(super) fn launch(launch: &Launch<'_>) -> io::Result<OwnedHandle> {
    let handles = launch.handles;
    let mut attributes = Attributes::new()?;
    let jobs = [launch.job.as_raw_handle()];
    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
    startup.StartupInfo.lpDesktop = launch.desktop.name.as_ptr().cast_mut();
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdInput = handles[0];
    startup.StartupInfo.hStdOutput = handles[1];
    startup.StartupInfo.hStdError = handles[2];
    let mut process = PROCESS_INFORMATION::default();
    let exe = wide(launch.executable)?;
    let environment = environment(launch.temp, launch.workspace, launch.cwd)?;
    let cwd = wide(launch.cwd)?;
    let mut command = command(launch.executable.as_os_str(), launch.args)?;
    // SAFETY: Every attribute value, string, stream handle, the token, and the job outlive CreateProcessAsUserW; JOB_LIST assigns containment before child code runs.
    unsafe {
        set_attributes(&mut attributes, handles, &jobs)?;
        startup.lpAttributeList = attributes.pointer();
        let created = CreateProcessAsUserW(
            launch.token.as_raw_handle(),
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
        boolean(created).map_err(|error| Error::other(format!("CreateProcessAsUserW: {error}")))?;
    }
    // SAFETY: A successful CreateProcessW transfers these two distinct handles to the caller.
    let (process, thread) = unsafe {
        (
            OwnedHandle::from_raw_handle(process.hProcess),
            OwnedHandle::from_raw_handle(process.hThread),
        )
    };
    drop(thread);
    Ok(process)
}

fn set_attributes(
    attributes: &mut Attributes,
    handles: &[HANDLE],
    jobs: &[HANDLE; 1],
) -> io::Result<()> {
    for (kind, pointer, size) in [
        (
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
            handles.as_ptr().cast(),
            std::mem::size_of_val(handles),
        ),
        (
            PROC_THREAD_ATTRIBUTE_JOB_LIST,
            jobs.as_ptr().cast(),
            size_of::<HANDLE>(),
        ),
    ] {
        // SAFETY: The caller keeps handles and jobs alive until CreateProcessW returns and the attribute list is deleted.
        boolean(unsafe {
            UpdateProcThreadAttribute(
                attributes.pointer(),
                0,
                kind as usize,
                pointer,
                size,
                null_mut(),
                null(),
            )
        })?;
    }
    Ok(())
}
