use super::*;

pub(super) const MODIFY: u32 =
    FILE_GENERIC_READ | FILE_GENERIC_EXECUTE | FILE_GENERIC_WRITE | DELETE;
pub(super) const GUARD: u32 = FILE_WRITE_DATA
    | FILE_APPEND_DATA
    | FILE_WRITE_EA
    | FILE_WRITE_ATTRIBUTES
    | FILE_DELETE_CHILD
    | DELETE;
pub(super) const DESKTOP_ALL: u32 = STANDARD_RIGHTS_REQUIRED
    | DESKTOP_READOBJECTS
    | DESKTOP_CREATEWINDOW
    | DESKTOP_CREATEMENU
    | DESKTOP_HOOKCONTROL
    | DESKTOP_JOURNALRECORD
    | DESKTOP_JOURNALPLAYBACK
    | DESKTOP_ENUMERATE
    | DESKTOP_WRITEOBJECTS
    | DESKTOP_SWITCHDESKTOP;

pub(super) struct Local(pub(super) *mut c_void);

impl Drop for Local {
    fn drop(&mut self) {
        // SAFETY: This guard owns a LocalAlloc result returned by a Win32 security API.
        unsafe { LocalFree(self.0) };
    }
}

pub(super) fn acl(path: &Path, sid: PSID, mask: u32, mode: ACCESS_MODE) -> io::Result<()> {
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
    if mode != REVOKE_ACCESS && present(old, sid, mask, mode)? {
        return Ok(());
    }
    let entry = EXPLICIT_ACCESS_W {
        grfAccessPermissions: mask,
        grfAccessMode: mode,
        grfInheritance: OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE,
        Trustee: TRUSTEE_W {
            TrusteeForm: TRUSTEE_IS_SID,
            ptstrName: sid.cast(),
            ..Default::default()
        },
    };
    let mut updated = null_mut();
    // SAFETY: sid outlives this call, descriptor owns old, and entry lives through this call.
    status(unsafe { SetEntriesInAclW(1, &entry, old, &mut updated) })?;
    let updated = Local(updated.cast());
    // SAFETY: updated owns the DACL; Windows propagates the inheritable entry to existing children.
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

fn present(dacl: *mut ACL, sid: PSID, mask: u32, mode: ACCESS_MODE) -> io::Result<bool> {
    if dacl.is_null() {
        return Ok(false);
    }
    let kind = if mode == DENY_ACCESS {
        ACCESS_DENIED_ACE_TYPE
    } else {
        ACCESS_ALLOWED_ACE_TYPE
    } as u8;
    let mut info = ACL_SIZE_INFORMATION::default();
    // SAFETY: dacl belongs to a live security descriptor, and info is correctly sized.
    boolean(unsafe {
        GetAclInformation(
            dacl,
            (&mut info as *mut ACL_SIZE_INFORMATION).cast(),
            size_of::<ACL_SIZE_INFORMATION>() as u32,
            AclSizeInformation,
        )
    })?;
    for index in 0..info.AceCount {
        let mut ace = null_mut();
        // SAFETY: index is below AceCount, and ace receives a pointer into dacl.
        boolean(unsafe { GetAce(dacl, index, &mut ace) })?;
        // SAFETY: Allowed and denied ACEs share this layout, with the SID starting at SidStart.
        let found = unsafe {
            let ace = &*ace.cast::<ACCESS_ALLOWED_ACE>();
            ace.Header.AceType == kind
                && u32::from(ace.Header.AceFlags) & INHERITED_ACE == 0
                && ace.Mask & mask == mask
                && EqualSid((&raw const ace.SidStart).cast_mut().cast(), sid) != 0
        };
        if found {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) struct Desktop {
    station: HWINSTA,
    desktop: HDESK,
    pub(super) name: Vec<u16>,
    sid: PSID,
}

impl Desktop {
    pub(super) fn create(sid: PSID, name: &str) -> io::Result<Self> {
        let mut text = null_mut();
        // SAFETY: The caller owns sid for the desktop's lifetime, and text receives a LocalAlloc string.
        boolean(unsafe { ConvertSidToStringSidW(sid, &mut text) })?;
        let text = Local(text.cast());
        let mut len = 0;
        // SAFETY: ConvertSidToStringSidW returns a terminated UTF-16 string.
        let sddl_sid = unsafe {
            let ptr = text.0.cast::<u16>();
            while *ptr.add(len) != 0 {
                len += 1;
            }
            String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len))
        };
        let sddl = wide(format!("D:(A;;GA;;;OW)(A;;GA;;;SY)(A;;GA;;;{sddl_sid})"))?;
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
            sid,
        };
        // SAFETY: Runs only in the sandbox subprocess; both UI objects have explicit ACLs and owned handles, and the station is restored.
        unsafe {
            desktop.station = CreateWindowStationW(
                null(),
                0,
                (WINSTA_READATTRIBUTES | WINSTA_CREATEDESKTOP) as u32 | READ_CONTROL | WRITE_DAC,
                &security,
            );
            if desktop.station.is_null() {
                return Err(Error::other(format!(
                    "CreateWindowStationW: {}",
                    Error::last_os_error()
                )));
            }
            station_acl(desktop.station, sid, false)?;
            let original = GetProcessWindowStation();
            boolean(SetProcessWindowStation(desktop.station))?;
            desktop.desktop = CreateDesktopW(
                desktop_name.as_ptr(),
                null(),
                null(),
                0,
                DESKTOP_ALL,
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

pub(super) fn station_acl(station: HWINSTA, sid: PSID, revoke: bool) -> io::Result<()> {
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
        grfAccessPermissions: READ_CONTROL
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
