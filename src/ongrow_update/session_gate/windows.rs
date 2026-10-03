use super::{Error, Product};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    os::windows::{ffi::OsStrExt, fs::OpenOptionsExt, io::AsRawHandle},
    path::{Component, Path, PathBuf, Prefix},
};
use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{CloseHandle, LocalFree, HANDLE, HLOCAL},
        Security::{
            Authorization::{GetSecurityInfo, SE_FILE_OBJECT},
            EqualSid, GetAce, GetAclInformation, GetTokenInformation, IsWellKnownSid,
            ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, ACL_SIZE_INFORMATION, AclSizeInformation,
            DACL_SECURITY_INFORMATION, INHERIT_ONLY_ACE, OWNER_SECURITY_INFORMATION,
            PSECURITY_DESCRIPTOR, PSID, TOKEN_QUERY, TOKEN_USER, TokenUser,
            WinBuiltinAdministratorsSid, WinLocalSystemSid,
        },
        Storage::FileSystem::{
            GetDriveTypeW, GetFileInformationByHandle, LockFileEx, UnlockFileEx,
            BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY, LOCK_FILE_FLAGS,
        },
        System::{IO::OVERLAPPED, Threading::{GetCurrentProcess, OpenProcessToken}},
        UI::Shell::{SHGetKnownFolderPath, FOLDERID_LocalAppData, FOLDERID_ProgramData, KF_FLAG_DEFAULT},
    },
};

const GATE: &str = "admission-v1.lock";
const JOURNAL: &str = "state-v1.journal";
const READY: u8 = 0;
const PENDING: u8 = 1;

// Fixed categories only. No path, SID, ACL, username or OS error is printed.
// The product build has no diagnostic output and rejects the same conditions.
fn untrusted(category: &'static str) -> Error {
    #[cfg(all(test, ongrow_session_gate_probe))]
    eprintln!("ONGROW_GATE_REJECT:{category}");
    let _ = category;
    Error::Untrusted
}

// CoTaskMemFree's existing crate feature is not enabled. Import this one SDK
// function directly rather than changing dependency features or the lockfile.
#[link(name = "ole32")]
extern "system" { fn CoTaskMemFree(memory: *const std::ffi::c_void); }

fn handle(file: &File) -> HANDLE { HANDLE(file.as_raw_handle()) }
fn wide(path: &Path) -> Vec<u16> { path.as_os_str().encode_wide().chain(Some(0)).collect() }
struct Token(HANDLE);
impl Drop for Token { fn drop(&mut self) { unsafe { let _ = CloseHandle(self.0); } } }
struct Descriptor(PSECURITY_DESCRIPTOR);
impl Drop for Descriptor { fn drop(&mut self) { unsafe { let _ = LocalFree(Some(HLOCAL(self.0.0))); } } }
struct User(Vec<usize>);
impl User {
    fn current() -> Result<Self, Error> {
        let mut token = Token(HANDLE::default());
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token.0) }.map_err(|_| untrusted("token-open"))?;
        let mut size = 0;
        let _ = unsafe { GetTokenInformation(token.0, TokenUser, None, 0, &mut size) };
        if size == 0 || size > 64 * 1024 { return Err(untrusted("token-size")); }
        let mut buffer = vec![0usize; (size as usize + std::mem::size_of::<usize>() - 1) / std::mem::size_of::<usize>()];
        unsafe { GetTokenInformation(token.0, TokenUser, Some(buffer.as_mut_ptr().cast()), size, &mut size) }.map_err(|_| untrusted("token-user"))?;
        Ok(Self(buffer))
    }
    fn sid(&self) -> PSID { unsafe { (*(self.0.as_ptr().cast::<TOKEN_USER>())).User.Sid } }
}
fn trusted_sid(sid: PSID, user: &User, product: Product, _ancestor: bool) -> bool {
    if sid.0.is_null() { return false; }
    unsafe {
        IsWellKnownSid(sid, WinLocalSystemSid).as_bool()
            || IsWellKnownSid(sid, WinBuiltinAdministratorsSid).as_bool()
            || (product == Product::SupportConsole && EqualSid(sid, user.sid()).is_ok())
    }
}

// Read-only diagnostics. Nothing in this module is available to product code.
#[cfg(all(test, ongrow_session_gate_probe))]
mod owner_diagnostics {
    use super::*;
    use windows::{core::w, Win32::Security::{
        Authorization::ConvertStringSidToSidW, IsValidSid,
        WinBuiltinUsersSid, WinWorldSid, WinCreatorOwnerSid, WinLocalServiceSid, WinNetworkServiceSid,
    }};

    struct AllocatedSid(PSID);
    impl Drop for AllocatedSid {
        fn drop(&mut self) { unsafe { let _ = LocalFree(Some(HLOCAL(self.0.0))); } }
    }
    fn trusted_installer() -> Option<AllocatedSid> {
        let mut value = AllocatedSid(PSID::default());
        // Exact public TrustedInstaller SID from Microsoft's WindowsAppSDK
        // ApplicationData specification, Machine Path/Folder. Classification only.
        unsafe { ConvertStringSidToSidW(w!("S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464"), &mut value.0) }.ok()?;
        if !unsafe { IsValidSid(value.0) }.as_bool() { return None; }
        Some(value)
    }
    fn all_services() -> Option<AllocatedSid> {
        let mut value = AllocatedSid(PSID::default());
        // Exact public All Services identity, not a prefix match for services.
        unsafe { ConvertStringSidToSidW(w!("S-1-5-80-0"), &mut value.0) }.ok()?;
        if !unsafe { IsValidSid(value.0) }.as_bool() { return None; }
        Some(value)
    }
    fn classify(sid: PSID, user: &User) -> &'static str {
        if sid.0.is_null() || !unsafe { IsValidSid(sid) }.as_bool() { return "other"; }
        if unsafe { IsWellKnownSid(sid, WinLocalSystemSid) }.as_bool() { return "system"; }
        if unsafe { IsWellKnownSid(sid, WinBuiltinAdministratorsSid) }.as_bool() { return "admins"; }
        for (kind, category) in [(WinBuiltinUsersSid, "builtin-users"), (WinWorldSid, "everyone"),
            (WinCreatorOwnerSid, "creator-owner"), (WinLocalServiceSid, "local-service"), (WinNetworkServiceSid, "network-service")] {
            if unsafe { IsWellKnownSid(sid, kind) }.as_bool() { return category; }
        }
        if unsafe { EqualSid(sid, user.sid()) }.is_ok() { return "current-user"; }
        if let Some(services) = all_services() {
            if unsafe { EqualSid(sid, services.0) }.is_ok() { return "all-services"; }
        }
        if let Some(installer) = trusted_installer() {
            if unsafe { EqualSid(sid, installer.0) }.is_ok() { return "trusted-installer"; }
        }
        "other"
    }
    pub(super) fn rejected(sid: PSID, user: &User) {
        eprintln!("ONGROW_GATE_OWNER:{}", classify(sid, user));
    }

    #[test]
    fn diagnostic_categories_never_grant_service_trust() {
        use windows::Win32::Security::{CreateWellKnownSid, WinNullSid};
        let user = User::current().unwrap();
        let installer = trusted_installer().unwrap();
        let services = all_services().unwrap();
        // Synthetic service SID differs only in the last subauthority. No prefix trust.
        let mut other_service = AllocatedSid(PSID::default());
        unsafe { ConvertStringSidToSidW(w!("S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478465"), &mut other_service.0) }.unwrap();
        assert!(unsafe { IsValidSid(other_service.0) }.as_bool());
        for (sid, category) in [(installer.0, "trusted-installer"), (services.0, "all-services"), (other_service.0, "other")] {
            assert_eq!(classify(sid, &user), category);
            for product in [Product::CustomerDesk, Product::SupportConsole] {
                for ancestor in [false, true] { assert!(!trusted_sid(sid, &user, product, ancestor)); }
            }
        }
        let public_classes = [(WinLocalSystemSid, "system"), (WinBuiltinAdministratorsSid, "admins"),
            (WinBuiltinUsersSid, "builtin-users"), (WinWorldSid, "everyone"), (WinCreatorOwnerSid, "creator-owner"),
            (WinLocalServiceSid, "local-service"), (WinNetworkServiceSid, "network-service"), (WinNullSid, "other")];
        for (kind, category) in public_classes {
            let mut buffer = [0usize; 16];
            let sid = PSID(buffer.as_mut_ptr().cast());
            let mut length = std::mem::size_of_val(&buffer) as u32;
            unsafe { CreateWellKnownSid(kind, None, Some(sid), &mut length) }.unwrap();
            assert_eq!(classify(sid, &user), category);
        }
        let expected_user = public_classes.iter().take(7)
            .find(|(kind, _)| unsafe { IsWellKnownSid(user.sid(), *kind) }.as_bool())
            .map(|(_, category)| *category).unwrap_or("current-user");
        assert_eq!(classify(user.sid(), &user), expected_user);
        assert_eq!(classify(PSID::default(), &user), "other");
    }
}

fn inspect(file: &File, user: &User, product: Product, directory: bool, ancestor: bool) -> Result<(), Error> {
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    unsafe { GetFileInformationByHandle(handle(file), &mut information) }.map_err(|_| untrusted("attributes-read"))?;
    if information.dwFileAttributes & 0x400 != 0
        || (information.dwFileAttributes & 0x10 != 0) != directory
        || (!directory && information.nNumberOfLinks != 1)
    { return Err(untrusted("attributes-or-hardlinks")); }
    let mut owner = PSID::default();
    let mut dacl: *mut ACL = std::ptr::null_mut();
    let mut descriptor = Descriptor(PSECURITY_DESCRIPTOR::default());
    let result = unsafe {
        GetSecurityInfo(handle(file), SE_FILE_OBJECT, OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            Some(&mut owner), None, Some(&mut dacl), None, Some(&mut descriptor.0))
    };
    if result.0 != 0 || descriptor.0.0.is_null() || dacl.is_null() { return Err(untrusted("security-descriptor")); }
    if !trusted_sid(owner, user, product, ancestor) {
        #[cfg(all(test, ongrow_session_gate_probe))]
        owner_diagnostics::rejected(owner, user);
        return Err(untrusted("owner-trust"));
    }
    if !ancestor && product == Product::SupportConsole && unsafe { EqualSid(owner, user.sid()) }.is_err() {
        return Err(untrusted("owner-exact-user"));
    }
    let mut size = ACL_SIZE_INFORMATION::default();
    unsafe { GetAclInformation(dacl, (&mut size as *mut ACL_SIZE_INFORMATION).cast(), std::mem::size_of::<ACL_SIZE_INFORMATION>() as u32, AclSizeInformation) }.map_err(|_| untrusted("acl-information"))?;
    for index in 0..size.AceCount {
        let mut pointer = std::ptr::null_mut();
        unsafe { GetAce(dacl, index, &mut pointer) }.map_err(|_| untrusted("acl-entry-read"))?;
        if pointer.is_null() { return Err(untrusted("acl-entry-null")); }
        let header = unsafe { &*pointer.cast::<ACE_HEADER>() };
        if header.AceFlags & INHERIT_ONLY_ACE.0 as u8 != 0 { continue; }
        if header.AceType == 1 { continue; } // ACCESS_DENIED_ACE_TYPE
        if header.AceType != 0 || (header.AceSize as usize) < std::mem::size_of::<ACCESS_ALLOWED_ACE>() { return Err(untrusted("acl-entry-type-or-size")); }
        let ace = unsafe { &*pointer.cast::<ACCESS_ALLOWED_ACE>() };
        let sid = PSID((&ace.SidStart as *const u32).cast_mut().cast());
        // Ancestors may permit creating siblings. They must not permit replacing
        // or reconfiguring existing protected children. The protected root and
        // both files reject any foreign write capability.
        let mutation = if ancestor { 0x000d_0150u32 | 0x5000_0000 } else { 0x000d_0156u32 | 0x5000_0000 };
        if ace.Mask & mutation != 0 && !trusted_sid(sid, user, product, ancestor) { return Err(untrusted("forbidden-access")); }
    }
    Ok(())
}

fn open(path: &Path, writable: bool, directory: bool, create: bool) -> Result<File, Error> {
    let mut options = OpenOptions::new();
    options.read(true).write(writable).create_new(create)
        // Hold every opened ancestor without FILE_SHARE_DELETE through the
        // lease. Neither an unprivileged rename nor a reparse swap is accepted.
        .share_mode(1 | 2)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0 | if directory { FILE_FLAG_BACKUP_SEMANTICS.0 } else { 0 });
    options.open(path).map_err(|error| if error.kind() == std::io::ErrorKind::NotFound { Error::MissingGate } else { untrusted(if directory { "directory-open" } else if create { "file-create" } else { "file-open" }) })
}
fn directory(path: &Path, user: &User, product: Product) -> Result<Vec<File>, Error> {
    let components: Vec<_> = path.components().collect();
    let Some(Component::Prefix(prefix)) = components.first() else { return Err(untrusted("root-prefix")); };
    let Prefix::Disk(letter) = prefix.kind() else { return Err(untrusted("root-drive-prefix")); };
    let volume = PathBuf::from(format!("{}:\\", letter as char));
    // No UNC, mapped network/removable drive, or reparse-mounted volume.
    if unsafe { GetDriveTypeW(PCWSTR(wide(&volume).as_ptr())) } != 3 { return Err(untrusted("root-drive-type")); }
    let mut current = PathBuf::new();
    let mut held = Vec::new();
    for (index, component) in components.iter().enumerate() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => current.push(component.as_os_str()),
            _ => return Err(untrusted("directory-component")),
        }
        if index == 0 { continue; }
        let file = open(&current, false, true, false)?;
        inspect(&file, user, product, true, index + 1 < components.len()).map_err(|error| {
            #[cfg(all(test, ongrow_session_gate_probe))]
            eprintln!("ONGROW_GATE_REJECT_CONTEXT:{}", if index == 1 { "root-volume" } else if index + 1 < components.len() { "ancestor" } else { "protected-root" });
            error
        })?;
        held.push(file);
    }
    if held.is_empty() { return Err(untrusted("directory-empty")); }
    Ok(held)
}
fn root(product: Product) -> Result<PathBuf, Error> {
    let id = if product == Product::CustomerDesk { &FOLDERID_ProgramData } else { &FOLDERID_LocalAppData };
    let pointer = unsafe { SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None) }.map_err(|_| Error::Untrusted)?;
    let value = unsafe { pointer.to_string() }.map_err(|_| Error::Untrusted);
    unsafe { CoTaskMemFree(pointer.0.cast()); }
    let suffix = if product == Product::CustomerDesk { "OnGROW\\Support Desk Update" } else { "OnGROW\\Support Console Update" };
    Ok(PathBuf::from(value?).join(suffix))
}

pub(super) struct Lock {
    gate: File,
    journal: File,
    _directories: Vec<File>,
    exclusive: bool,
}
impl Drop for Lock {
    fn drop(&mut self) {
        unsafe { let _ = UnlockFileEx(handle(&self.gate), None, 1, 0, &mut OVERLAPPED::default()); }
    }
}
impl Lock {
    fn state(&mut self) -> Result<u8, Error> {
        if self.journal.metadata().map_err(|_| Error::Untrusted)?.len() != 1 { return Err(Error::Untrusted); }
        self.journal.seek(SeekFrom::Start(0)).map_err(|_| Error::Io)?;
        let mut value = [0];
        self.journal.read_exact(&mut value).map_err(|_| Error::Untrusted)?;
        if value[0] != READY && value[0] != PENDING { return Err(Error::Untrusted); }
        Ok(value[0])
    }
    fn write_state(&mut self, state: u8) -> Result<(), Error> {
        if !self.exclusive { return Err(Error::Untrusted); }
        self.state()?;
        self.journal.seek(SeekFrom::Start(0)).map_err(|_| Error::Io)?;
        self.journal.write_all(&[state]).map_err(|_| Error::Io)?;
        // File::sync_all calls FlushFileBuffers on this fixed journal handle.
        self.journal.sync_all().map_err(|_| Error::Io)
    }
    pub(super) fn mark_pending(&mut self) -> Result<(), Error> {
        if self.state()? == PENDING { return Err(Error::Pending); }
        self.write_state(PENDING)
    }
    pub(super) fn mark_ready(&mut self) -> Result<(), Error> { self.write_state(READY) }
}

fn acquire(path: &Path, product: Product, exclusive: bool) -> Result<Lock, Error> {
    let user = User::current()?;
    let directories = directory(path, &user, product)?;
    let gate = open(&path.join(GATE), exclusive, false, false)?;
    inspect(&gate, &user, product, false, false)?;
    if gate.metadata().map_err(|_| Error::Untrusted)?.len() != 0 { return Err(Error::Untrusted); }
    let flag = LOCK_FILE_FLAGS(LOCKFILE_FAIL_IMMEDIATELY.0 | if exclusive { LOCKFILE_EXCLUSIVE_LOCK.0 } else { 0 });
    if let Err(error) = unsafe { LockFileEx(handle(&gate), flag, None, 1, 0, &mut OVERLAPPED::default()) } {
        return Err(if error.code().0 as u32 == 0x8007_0021 { Error::Busy } else { Error::Io });
    }
    let journal = open(&path.join(JOURNAL), exclusive, false, false)?;
    inspect(&journal, &user, product, false, false)?;
    let mut lock = Lock { gate, journal, _directories: directories, exclusive };
    if lock.state()? == PENDING && !exclusive { return Err(Error::Pending); }
    Ok(lock)
}
pub(super) fn admit(product: Product) -> Result<Lock, Error> { acquire(&root(product)?, product, false) }
pub(super) fn exclusive(product: Product) -> Result<Lock, Error> { acquire(&root(product)?, product, true) }
pub(super) fn initialize(product: Product) -> Result<(), Error> { initialize_at(&root(product)?, product) }
fn initialize_at(path: &Path, product: Product) -> Result<(), Error> {
    let user = User::current()?;
    let _directories = directory(path, &user, product)?;
    for name in [GATE, JOURNAL] {
        match std::fs::symlink_metadata(path.join(name)) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
            _ => return Err(untrusted("bootstrap-state-already-present")),
        }
    }
    let mut journal = open(&path.join(JOURNAL), true, false, true)?;
    inspect(&journal, &user, product, false, false).map_err(|error| {
        #[cfg(all(test, ongrow_session_gate_probe))]
        eprintln!("ONGROW_GATE_REJECT_CONTEXT:journal-bootstrap");
        error
    })?;
    journal.write_all(&[READY]).map_err(|_| Error::Io)?;
    journal.sync_all().map_err(|_| Error::Io)?;
    let gate = open(&path.join(GATE), true, false, true)?;
    inspect(&gate, &user, product, false, false).map_err(|error| {
        #[cfg(all(test, ongrow_session_gate_probe))]
        eprintln!("ONGROW_GATE_REJECT_CONTEXT:gate-bootstrap");
        error
    })?;
    gate.sync_all().map_err(|_| Error::Io)?;
    Ok(())
}
#[cfg(test)]
pub(super) fn fixture_initialize(path: &Path) -> Result<(), Error> { initialize_at(path, Product::SupportConsole) }
#[cfg(test)]
pub(super) fn fixture_acquire(path: &Path, exclusive: bool) -> Result<Lock, Error> { acquire(path, Product::SupportConsole, exclusive) }
