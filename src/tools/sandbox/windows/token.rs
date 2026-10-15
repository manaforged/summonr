use sha2::{Digest, Sha256};

use super::*;

const NT_AUTHORITY: u8 = 5;
const WORLD_AUTHORITY: u8 = 1;
const OWNER_RIGHTS_AUTHORITY: u8 = 3;
const DOMAIN_PREFIX: u32 = 21;
const OWNER_RIGHTS: u32 = 4;

pub(super) struct Sid(Vec<u32>);

impl Sid {
    fn new(authority: u8, parts: &[u32]) -> Self {
        let mut words = vec![0_u32; 2 + parts.len()];
        let head = [1, parts.len() as u8, 0, 0, 0, 0, 0, authority];
        words[0] = u32::from_le_bytes([head[0], head[1], head[2], head[3]]);
        words[1] = u32::from_le_bytes([head[4], head[5], head[6], head[7]]);
        words[2..].copy_from_slice(parts);
        Self(words)
    }

    pub(super) fn for_path(path: &Path) -> Self {
        let key = simplified(path).to_string_lossy().to_lowercase();
        let digest = Sha256::digest(key.as_bytes());
        let mut parts = [DOMAIN_PREFIX, 0, 0, 0, 0];
        for (part, chunk) in parts[1..].iter_mut().zip(digest.as_chunks::<4>().0) {
            *part = u32::from_le_bytes(*chunk);
        }
        Self::new(NT_AUTHORITY, &parts)
    }

    fn world() -> Self {
        Self::new(WORLD_AUTHORITY, &[0])
    }

    fn owner_rights() -> Self {
        Self::new(OWNER_RIGHTS_AUTHORITY, &[OWNER_RIGHTS])
    }

    fn copy(sid: PSID) -> Self {
        // SAFETY: The caller passes a SID owned by a live token information buffer.
        let len = unsafe { GetLengthSid(sid) } as usize;
        let mut words = vec![0_u32; len.div_ceil(4)];
        // SAFETY: words holds at least len bytes, and sid is valid for len bytes.
        unsafe { std::ptr::copy_nonoverlapping(sid.cast::<u8>(), words.as_mut_ptr().cast(), len) };
        Self(words)
    }

    pub(super) fn ptr(&self) -> PSID {
        self.0.as_ptr().cast_mut().cast()
    }
}

pub(super) struct Restricted {
    pub(super) token: OwnedHandle,
    pub(super) logon: Sid,
}

pub(super) fn restricted(roots: &[Sid]) -> io::Result<Restricted> {
    let mut raw = null_mut();
    // SAFETY: The current-process pseudo handle is valid, and raw receives an owned token.
    boolean(unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_DUPLICATE | TOKEN_QUERY | TOKEN_ASSIGN_PRIMARY | TOKEN_ADJUST_DEFAULT,
            &mut raw,
        )
    })?;
    // SAFETY: OpenProcessToken returned an exclusively owned handle.
    let base = unsafe { OwnedHandle::from_raw_handle(raw) };
    let logon = logon_sid(&base)?;
    let world = Sid::world();
    let mut entries: Vec<SID_AND_ATTRIBUTES> = roots
        .iter()
        .chain([&logon, &world])
        .map(|sid| SID_AND_ATTRIBUTES {
            Sid: sid.ptr(),
            Attributes: 0,
        })
        .collect();
    let mut token = null_mut();
    // SAFETY: entries borrows SIDs that outlive this call, and token receives an owned handle.
    boolean(unsafe {
        CreateRestrictedToken(
            base.as_raw_handle(),
            DISABLE_MAX_PRIVILEGE | WRITE_RESTRICTED,
            0,
            null(),
            0,
            null(),
            entries.len() as u32,
            entries.as_mut_ptr(),
            &mut token,
        )
    })
    .map_err(|error| Error::other(format!("CreateRestrictedToken: {error}")))?;
    // SAFETY: CreateRestrictedToken returned an exclusively owned handle.
    let token = unsafe { OwnedHandle::from_raw_handle(token) };
    default_dacl(&token, &logon)?;
    Ok(Restricted { token, logon })
}

fn logon_sid(token: &OwnedHandle) -> io::Result<Sid> {
    let mut needed = 0;
    // SAFETY: A null buffer asks only for the TokenGroups size.
    unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenGroups,
            null_mut(),
            0,
            &mut needed,
        )
    };
    let mut buffer = vec![0_u64; (needed as usize).div_ceil(8)];
    // SAFETY: buffer is aligned and at least needed bytes long.
    boolean(unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenGroups,
            buffer.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    })?;
    // SAFETY: GetTokenInformation filled buffer with a TOKEN_GROUPS header and Count entries.
    let groups = unsafe {
        let header = &*buffer.as_ptr().cast::<TOKEN_GROUPS>();
        std::slice::from_raw_parts(header.Groups.as_ptr(), header.GroupCount as usize)
    };
    let logon = SE_GROUP_LOGON_ID as u32;
    groups
        .iter()
        .find(|group| group.Attributes & logon == logon)
        .map(|group| Sid::copy(group.Sid))
        .ok_or_else(|| Error::other("the session token has no logon SID"))
}

fn default_dacl(token: &OwnedHandle, logon: &Sid) -> io::Result<()> {
    let owner = Sid::owner_rights();
    let entries =
        [(logon, GENERIC_ALL), (&owner, READ_CONTROL)].map(|(sid, mask)| EXPLICIT_ACCESS_W {
            grfAccessPermissions: mask,
            grfAccessMode: GRANT_ACCESS,
            grfInheritance: 0,
            Trustee: TRUSTEE_W {
                TrusteeForm: TRUSTEE_IS_SID,
                ptstrName: sid.ptr().cast(),
                ..Default::default()
            },
        });
    let mut dacl = null_mut();
    // SAFETY: entries borrow SIDs that outlive the call, and dacl receives a LocalAlloc ACL.
    status(unsafe { SetEntriesInAclW(entries.len() as u32, entries.as_ptr(), null(), &mut dacl) })?;
    let dacl = Local(dacl.cast());
    let info = TOKEN_DEFAULT_DACL {
        DefaultDacl: dacl.0.cast(),
    };
    // SAFETY: The token handle is live and info points at a valid ACL for the call.
    boolean(unsafe {
        SetTokenInformation(
            token.as_raw_handle(),
            TokenDefaultDacl,
            (&info as *const TOKEN_DEFAULT_DACL).cast(),
            size_of::<TOKEN_DEFAULT_DACL>() as u32,
        )
    })
}
