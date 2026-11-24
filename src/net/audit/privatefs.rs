use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::Path;
fn permission() -> io::Error {
    io::Error::from(io::ErrorKind::PermissionDenied)
}
fn plain_entry(path: &Path) -> io::Result<fs::Metadata> {
    let info = fs::symlink_metadata(path)?;
    let kind = info.file_type();
    if kind.is_symlink() || !(kind.is_file() || kind.is_dir()) {
        return Err(permission());
    }
    Ok(info)
}
pub(crate) fn check(path: &Path) -> io::Result<()> {
    let info = plain_entry(path)?;
    if !sys::is_private(path, &info)? {
        return Err(permission());
    }
    Ok(())
}
pub(crate) fn secure(path: &Path) -> io::Result<()> {
    let info = plain_entry(path)?;
    sys::protect(path, &info)
}
pub(crate) fn create_directory(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            sys::create_dir_all(path)?;
            secure(path)
        }
        Err(e) => Err(e),
        Ok(info) if !info.is_dir() => Err(permission()),
        Ok(_) => check(path),
    }
}
pub(crate) fn replace(from: &Path, to: &Path) -> io::Result<()> {
    sys::replace(from, to)
}
pub(crate) fn sync_directory(dir: &Path) -> io::Result<()> {
    sys::sync_directory(dir)
}
pub(crate) fn lock_spool(path: &Path) -> io::Result<File> {
    let f = sys::open_lock(path)?;
    if fs4::fs_std::FileExt::try_lock_exclusive(&f)? {
        Ok(f)
    } else {
        Err(io::Error::from(io::ErrorKind::WouldBlock))
    }
}
pub(crate) fn unlock_spool(f: File) -> io::Result<()> {
    fs4::fs_std::FileExt::unlock(&f)
}
pub(crate) fn hostname() -> String {
    sys::hostname()
}
pub(crate) fn create_temp(dir: &Path) -> io::Result<(File, std::path::PathBuf)> {
    use rand_core::RngCore;
    for _ in 0..10000 {
        let n = rand_core::OsRng.next_u32();
        let path = dir.join(format!(".tmp-{n}"));
        match sys::create_new(&path) {
            Ok(f) => return Ok((f, path)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::from(io::ErrorKind::AlreadyExists))
}
#[cfg(unix)]
mod sys {
    #![forbid(unsafe_code)]
    use super::*;
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
    pub(super) fn protect(path: &Path, info: &fs::Metadata) -> io::Result<()> {
        let mode = if info.is_dir() { 0o700 } else { 0o600 };
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
    }
    pub(super) fn is_private(_: &Path, info: &fs::Metadata) -> io::Result<bool> {
        Ok(info.permissions().mode() & 0o077 == 0)
    }
    pub(super) fn create_dir_all(path: &Path) -> io::Result<()> {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)
    }
    pub(super) fn replace(from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }
    pub(super) fn sync_directory(dir: &Path) -> io::Result<()> {
        File::open(dir)?.sync_all()
    }
    pub(super) fn open_lock(path: &Path) -> io::Result<File> {
        OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(path)
    }
    pub(super) fn create_new(path: &Path) -> io::Result<File> {
        OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
    }
    pub(super) fn hostname() -> String {
        let name = rustix::system::uname();
        name.nodename().to_string_lossy().into_owned()
    }
}
#[cfg(windows)]
#[allow(unsafe_code)]
mod sys {
    use super::*;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LocalFree};
    use windows_sys::Win32::Security::Authorization::{
        EXPLICIT_ACCESS_W, GetNamedSecurityInfoW, NO_MULTIPLE_TRUSTEE, SE_FILE_OBJECT, SET_ACCESS,
        SetEntriesInAclW, SetNamedSecurityInfoW, TRUSTEE_IS_SID, TRUSTEE_IS_USER, TRUSTEE_W,
    };
    use windows_sys::Win32::Security::{
        ACCESS_ALLOWED_ACE, ACL, CreateWellKnownSid, DACL_SECURITY_INFORMATION, EqualSid, GetAce,
        GetSecurityDescriptorControl, GetTokenInformation, INHERITED_ACE, NO_INHERITANCE,
        OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSID, SE_DACL_PROTECTED,
        TOKEN_QUERY, TOKEN_USER, TokenUser, WinCreatorOwnerRightsSid, WinLocalSystemSid,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    use windows_sys::Win32::System::SystemInformation::{
        ComputerNamePhysicalDnsHostname, GetComputerNameExW,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    const GENERIC_ALL: u32 = 0x1000_0000;
    const FILE_ALL_ACCESS: u32 = 0x001f_01ff;
    const ACCESS_ALLOWED_ACE_TYPE: u8 = 0;
    const MAX_SID_BYTES: usize = 68;
    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }
    fn win32(code: u32) -> io::Result<()> {
        if code == 0 {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(code as i32))
        }
    }
    struct Sid(Vec<u64>);
    impl Sid {
        fn ptr(&self) -> PSID {
            self.0.as_ptr() as PSID
        }
    }
    fn well_known(kind: i32) -> io::Result<Sid> {
        let mut buffer = vec![0u64; MAX_SID_BYTES.div_ceil(8)];
        let mut size = (buffer.len() * 8) as u32;
        let ok =
            unsafe { CreateWellKnownSid(kind, null_mut(), buffer.as_mut_ptr().cast(), &mut size) };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Sid(buffer))
    }
    struct UserSid(Vec<u64>);
    impl UserSid {
        fn ptr(&self) -> PSID {
            unsafe { (*(self.0.as_ptr() as *const TOKEN_USER)).User.Sid }
        }
    }
    fn user_sid() -> io::Result<UserSid> {
        let mut token: HANDLE = null_mut();
        unsafe {
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return Err(io::Error::last_os_error());
            }
            let mut buffer = vec![0u64; 64];
            let mut size = 0u32;
            let ok = GetTokenInformation(
                token,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                (buffer.len() * 8) as u32,
                &mut size,
            );
            let err = io::Error::last_os_error();
            CloseHandle(token);
            if ok == 0 {
                return Err(err);
            }
            Ok(UserSid(buffer))
        }
    }
    unsafe fn equal(a: PSID, b: PSID) -> bool {
        unsafe { EqualSid(a, b) != 0 }
    }
    pub(super) fn protect(path: &Path, info: &fs::Metadata) -> io::Result<()> {
        let mut permissions = info.permissions();
        if permissions.readonly() {
            #[allow(clippy::permissions_set_readonly_false)]
            permissions.set_readonly(false);
            fs::set_permissions(path, permissions)?;
        }
        let user = user_sid()?;
        let system = well_known(WinLocalSystemSid)?;
        let mut sids = vec![user.ptr()];
        if !unsafe { equal(user.ptr(), system.ptr()) } {
            sids.push(system.ptr());
        }
        let access: Vec<EXPLICIT_ACCESS_W> = sids
            .iter()
            .map(|sid| EXPLICIT_ACCESS_W {
                grfAccessPermissions: GENERIC_ALL,
                grfAccessMode: SET_ACCESS,
                grfInheritance: NO_INHERITANCE,
                Trustee: TRUSTEE_W {
                    pMultipleTrustee: null_mut(),
                    MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
                    TrusteeForm: TRUSTEE_IS_SID,
                    TrusteeType: TRUSTEE_IS_USER,
                    ptstrName: (*sid).cast(),
                },
            })
            .collect();
        let mut acl: *mut ACL = null_mut();
        let name = wide(path);
        unsafe {
            win32(SetEntriesInAclW(
                access.len() as u32,
                access.as_ptr(),
                null(),
                &mut acl,
            ))?;
            let code = SetNamedSecurityInfoW(
                name.as_ptr(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION
                    | DACL_SECURITY_INFORMATION
                    | PROTECTED_DACL_SECURITY_INFORMATION,
                user.ptr(),
                null_mut(),
                acl,
                null(),
            );
            LocalFree(acl.cast());
            win32(code)
        }
    }
    pub(super) fn is_private(path: &Path, _: &fs::Metadata) -> io::Result<bool> {
        let name = wide(path);
        let mut owner: PSID = null_mut();
        let mut dacl: *mut ACL = null_mut();
        let mut descriptor = null_mut();
        unsafe {
            win32(GetNamedSecurityInfoW(
                name.as_ptr(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut owner,
                null_mut(),
                &mut dacl,
                null_mut(),
                &mut descriptor,
            ))?;
        }
        struct Free(*mut core::ffi::c_void);
        impl Drop for Free {
            fn drop(&mut self) {
                unsafe {
                    LocalFree(self.0);
                }
            }
        }
        let _free = Free(descriptor);
        let mut control = 0u16;
        let mut revision = 0u32;
        if unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if control & SE_DACL_PROTECTED == 0 || dacl.is_null() {
            return Ok(false);
        }
        let user = user_sid()?;
        let system = well_known(WinLocalSystemSid)?;
        if owner.is_null() || !unsafe { equal(owner, user.ptr()) } {
            return Ok(false);
        }
        let owner_rights = well_known(WinCreatorOwnerRightsSid)?;
        let system_differs = !unsafe { equal(user.ptr(), system.ptr()) };
        let allowed = if system_differs { 2 } else { 1 };
        let count = unsafe { (*dacl).AceCount } as usize;
        if count < 1 || count > allowed {
            return Ok(false);
        }
        let (mut seen_user, mut seen_system) = (false, false);
        for index in 0..count {
            let mut ace: *mut core::ffi::c_void = null_mut();
            if unsafe { GetAce(dacl, index as u32, &mut ace) } == 0 {
                return Err(io::Error::last_os_error());
            }
            if ace.is_null() {
                return Ok(false);
            }
            let ace = ace as *const ACCESS_ALLOWED_ACE;
            let (kind, flags) = unsafe { ((*ace).Header.AceType, (*ace).Header.AceFlags) };
            if kind != ACCESS_ALLOWED_ACE_TYPE || u32::from(flags) & INHERITED_ACE != 0 {
                return Ok(false);
            }
            let (mask, sid) = unsafe { ((*ace).Mask, (&raw const (*ace).SidStart) as PSID) };
            if mask & GENERIC_ALL == 0 && mask & FILE_ALL_ACCESS != FILE_ALL_ACCESS {
                return Ok(false);
            }
            let (is_user, is_system) = unsafe {
                (
                    equal(sid, owner_rights.ptr()) || equal(sid, user.ptr()),
                    system_differs && equal(sid, system.ptr()),
                )
            };
            let seen = if is_user {
                &mut seen_user
            } else if is_system {
                &mut seen_system
            } else {
                return Ok(false);
            };
            if *seen {
                return Ok(false);
            }
            *seen = true;
        }
        let _ = seen_system;
        Ok(seen_user)
    }
    pub(super) fn create_dir_all(path: &Path) -> io::Result<()> {
        fs::create_dir_all(path)
    }
    pub(super) fn replace(from: &Path, to: &Path) -> io::Result<()> {
        let (source, target) = (wide(from), wide(to));
        let ok = unsafe {
            MoveFileExW(
                source.as_ptr(),
                target.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
    pub(super) fn sync_directory(_: &Path) -> io::Result<()> {
        Ok(())
    }
    pub(super) fn open_lock(path: &Path) -> io::Result<File> {
        OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
    }
    pub(super) fn create_new(path: &Path) -> io::Result<File> {
        OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path)
    }
    pub(super) fn hostname() -> String {
        let mut size = 0u32;
        unsafe {
            GetComputerNameExW(ComputerNamePhysicalDnsHostname, null_mut(), &mut size);
            if size == 0 {
                return String::new();
            }
            let mut buffer = vec![0u16; size as usize];
            if GetComputerNameExW(
                ComputerNamePhysicalDnsHostname,
                buffer.as_mut_ptr(),
                &mut size,
            ) == 0
            {
                return String::new();
            }
            String::from_utf16_lossy(&buffer[..size as usize])
        }
    }
}
#[cfg(not(any(unix, windows)))]
mod sys {
    #![forbid(unsafe_code)]
    use super::*;
    pub(super) fn protect(_: &Path, _: &fs::Metadata) -> io::Result<()> {
        Ok(())
    }
    pub(super) fn is_private(_: &Path, _: &fs::Metadata) -> io::Result<bool> {
        Ok(true)
    }
    pub(super) fn create_dir_all(path: &Path) -> io::Result<()> {
        fs::create_dir_all(path)
    }
    pub(super) fn replace(from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }
    pub(super) fn sync_directory(_: &Path) -> io::Result<()> {
        Ok(())
    }
    pub(super) fn open_lock(path: &Path) -> io::Result<File> {
        OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
    }
    pub(super) fn create_new(path: &Path) -> io::Result<File> {
        OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path)
    }
    pub(super) fn hostname() -> String {
        String::new()
    }
}
