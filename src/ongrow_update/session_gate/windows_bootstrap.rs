//! Fixed Windows Desk bootstrap and operational file access.
use super::*;
pub(crate) fn require_system() -> Result<(), Error> {
    if unsafe { IsWellKnownSid(User::current()?.sid(), WinLocalSystemSid) }.as_bool() {
        Ok(())
    } else {
        Err(Error::Untrusted)
    }
}
fn known_folder(id: &windows::core::GUID) -> Result<PathBuf, Error> {
    let pointer =
        unsafe { SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None) }.map_err(|_| Error::Untrusted)?;
    let value = unsafe { pointer.to_string() }.map_err(|_| Error::Untrusted);
    unsafe {
        CoTaskMemFree(pointer.0.cast());
    }
    Ok(PathBuf::from(value?))
}
pub(crate) fn desk_installation() -> Result<PathBuf, Error> {
    Ok(
        known_folder(&windows::Win32::UI::Shell::FOLDERID_ProgramFilesX64)?
            .join("OnGROW Support Desk"),
    )
}
pub(crate) fn installed_file(name: &str) -> Result<File, Error> {
    if !matches!(
        name,
        "ongrow-update.exe" | "OnGROW Support Desk.exe" | "librustdesk.dll"
    ) {
        return Err(Error::Untrusted);
    }
    let path = desk_installation()?;
    let user = User::current()?;
    let _parents = directory(&path, &user, Product::CustomerDesk)?;
    let file = open(&path.join(name), false, false, false)?;
    inspect(&file, &user, Product::CustomerDesk, false, false)?;
    Ok(file)
}
pub(crate) fn system_executable(name: &str) -> Result<PathBuf, Error> {
    if name != "msiexec.exe" {
        return Err(Error::Untrusted);
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetSystemDirectoryW(buffer: *mut u16, size: u32) -> u32;
    }
    let mut buffer = [0u16; 32768];
    let size = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    if size == 0 || size >= buffer.len() {
        return Err(Error::Untrusted);
    }
    let path = PathBuf::from(String::from_utf16(&buffer[..size]).map_err(|_| Error::Untrusted)?);
    let user = User::current()?;
    let mut parents = Vec::new();
    let mut current = PathBuf::new();
    for (index, component) in path.components().enumerate() {
        current.push(component);
        if index == 0 {
            continue;
        }
        let file = open(&current, false, true, false)?;
        inspect(&file, &user, Product::CustomerDesk, true, true)?;
        parents.push(file);
    }
    let file = open(&path.join(name), false, false, false)?;
    inspect(&file, &user, Product::CustomerDesk, false, true)?;
    // Ancestor policy allows sibling creation. A system executable must also
    // reject foreign writes to its own data, while retaining exact TI trust.
    let mut owner = PSID::default();
    let mut dacl: *mut ACL = std::ptr::null_mut();
    let mut descriptor = Descriptor(PSECURITY_DESCRIPTOR::default());
    let status = unsafe {
        GetSecurityInfo(
            handle(&file),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            Some(&mut owner),
            None,
            Some(&mut dacl),
            None,
            Some(&mut descriptor.0),
        )
    };
    if status.0 != 0 || dacl.is_null() {
        return Err(Error::Untrusted);
    }
    let mut size = ACL_SIZE_INFORMATION::default();
    unsafe {
        GetAclInformation(
            dacl,
            (&mut size as *mut ACL_SIZE_INFORMATION).cast(),
            std::mem::size_of::<ACL_SIZE_INFORMATION>() as u32,
            AclSizeInformation,
        )
    }
    .map_err(|_| Error::Untrusted)?;
    for index in 0..size.AceCount {
        let mut pointer = std::ptr::null_mut();
        unsafe { GetAce(dacl, index, &mut pointer) }.map_err(|_| Error::Untrusted)?;
        if pointer.is_null() {
            return Err(Error::Untrusted);
        }
        let header = unsafe { &*pointer.cast::<ACE_HEADER>() };
        if header.AceType != 0 || header.AceFlags & INHERIT_ONLY_ACE.0 as u8 != 0 {
            continue;
        }
        let ace = unsafe { &*pointer.cast::<ACCESS_ALLOWED_ACE>() };
        let sid = PSID((&ace.SidStart as *const u32).cast_mut().cast());
        if ace.Mask & 6 != 0 && !trusted_ace_sid(sid, owner, &user, Product::CustomerDesk, true) {
            return Err(Error::Untrusted);
        }
    }
    Ok(path.join(name))
}
// First-run creation by the LocalSystem product service only. Existing security
// descriptors are checked, never repaired; partial bootstrap stays blocked.
pub(crate) fn bootstrap_desk_root() -> Result<bool, Error> {
    require_system()?;
    let path = root(Product::CustomerDesk)?;
    if path.exists() {
        ProtectedRoot::open(Product::CustomerDesk)?;
        return Ok(false);
    }
    let user = User::current()?;
    let mut descriptor = Descriptor(PSECURITY_DESCRIPTOR::default());
    unsafe { windows::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW(
        w!("O:SYG:SYD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;GRGX;;;AU)"), 1,
        &mut descriptor.0, None) }.map_err(|_| Error::Untrusted)?;
    let attributes = windows::Win32::Security::SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<windows::Win32::Security::SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0 .0,
        bInheritHandle: windows::core::BOOL(0),
    };
    let mut held = Vec::new();
    let mut current = PathBuf::new();
    for (index, component) in path.components().enumerate() {
        current.push(component);
        if index == 0 {
            continue;
        }
        let file = match open(&current, false, true, false) {
            Ok(file) => file,
            Err(Error::MissingGate) => {
                // Only the fixed OnGROW parent and product root may be created.
                if current != path && Some(current.as_path()) != path.parent() {
                    return Err(Error::Untrusted);
                }
                unsafe {
                    windows::Win32::Storage::FileSystem::CreateDirectoryW(
                        PCWSTR(wide(&current).as_ptr()),
                        Some(&attributes),
                    )
                }
                .map_err(|_| Error::Untrusted)?;
                open(&current, false, true, false)?
            }
            Err(error) => return Err(error),
        };
        inspect(&file, &user, Product::CustomerDesk, true, current != path)?;
        held.push(file);
    }
    ProtectedRoot::open(Product::CustomerDesk)?;
    Ok(true)
}
impl ProtectedRoot {
    pub(crate) fn path(&self, child: Child) -> PathBuf {
        self.path.join(child.name())
    }
    pub(crate) fn replace(&self, child: Child, bytes: &[u8]) -> Result<(), Error> {
        require_system()?;
        if child == Child::Sequence {
            return self.replace_record(Child::Sequence, Child::SequenceNext, 56, bytes);
        }
        if child == Child::NativeState {
            return self.replace_record(Child::NativeState, Child::NativeNext, 160, bytes);
        }
        if !matches!(child, Child::Health) {
            return Err(Error::Untrusted);
        }
        let mut file = match self.read(child) {
            Ok(file) => {
                drop(file);
                OpenOptions::new()
                    .read(true)
                    .write(true)
                    .share_mode(1)
                    .custom_flags(
                        FILE_FLAG_OPEN_REPARSE_POINT.0
                            | windows::Win32::Storage::FileSystem::FILE_FLAG_WRITE_THROUGH.0,
                    )
                    .open(self.path(child))
                    .map_err(|_| Error::Io)?
            }
            Err(Error::MissingGate) if matches!(child, Child::Health) => self.create(child)?,
            Err(error) => return Err(error),
        };
        self.identity(&file)?;
        file.seek(SeekFrom::Start(0)).map_err(|_| Error::Io)?;
        file.write_all(bytes).map_err(|_| Error::Io)?;
        file.set_len(bytes.len() as u64).map_err(|_| Error::Io)?;
        self.sync_file(&file)
    }
    fn replace_record(
        &self,
        child: Child,
        next_child: Child,
        size: usize,
        bytes: &[u8],
    ) -> Result<(), Error> {
        if bytes.len() != size {
            return Err(Error::Untrusted);
        }
        let current = self.read(child)?;
        self.identity(&current)?;
        let next = match self.create(next_child) {
            Ok(mut writer) => {
                writer.write_all(bytes).map_err(|_| Error::Io)?;
                self.sync_file(&writer)?;
                drop(writer);
                self.read(next_child)?
            }
            Err(_) => {
                let mut next = self.read(next_child)?;
                if next.metadata().map_err(|_| Error::Io)?.len() != size as u64 {
                    return Err(Error::Untrusted);
                }
                let mut stored = vec![0; size];
                next.read_exact(&mut stored).map_err(|_| Error::Io)?;
                if stored != bytes {
                    return Err(Error::Untrusted);
                }
                next
            }
        };
        self.identity(&next)?;
        drop(next);
        drop(current);
        // Same-directory atomic replacement, flushed before any Ready marker.
        unsafe {
            windows::Win32::Storage::FileSystem::MoveFileExW(
                PCWSTR(wide(&self.path(next_child)).as_ptr()),
                PCWSTR(wide(&self.path(child)).as_ptr()),
                windows::Win32::Storage::FileSystem::MOVEFILE_REPLACE_EXISTING
                    | windows::Win32::Storage::FileSystem::MOVEFILE_WRITE_THROUGH,
            )
        }
        .map_err(|_| Error::Io)?;
        let mut committed = self.read(child)?;
        let mut stored = vec![0; size];
        committed.read_exact(&mut stored).map_err(|_| Error::Io)?;
        if stored != bytes {
            return Err(Error::Untrusted);
        }
        Ok(())
    }
    pub(crate) fn remove(&self, child: Child) -> Result<(), Error> {
        require_system()?;
        if !matches!(
            child,
            Child::Payload
                | Child::Attempt
                | Child::Guardian
                | Child::Installer
                | Child::NativeState
                | Child::NativeNext
        ) {
            return Err(Error::Untrusted);
        }
        let file = self.read(child)?;
        self.identity(&file)?;
        drop(file);
        // Every ancestor remains pinned; only SYSTEM/admin can alter the child.
        std::fs::remove_file(self.path(child)).map_err(|_| Error::Io)
    }
}

pub(crate) fn pending() -> Result<bool, Error> {
    let path = root(Product::CustomerDesk)?;
    let user = User::current()?;
    let _parents = directory(&path, &user, Product::CustomerDesk)?;
    let mut file = open(&path.join(JOURNAL), false, false, false)?;
    inspect(&file, &user, Product::CustomerDesk, false, false)?;
    if file.metadata().map_err(|_| Error::Io)?.len() != 1 {
        return Err(Error::Untrusted);
    }
    let mut byte = [0];
    file.read_exact(&mut byte).map_err(|_| Error::Io)?;
    match byte[0] {
        READY => Ok(false),
        PENDING => Ok(true),
        _ => Err(Error::Untrusted),
    }
}

// Manifest entries may select only regular runtime files under this fixed app
// tree. No config/private path, reparse point, link, ADS or Windows alias is read.
pub(crate) fn installed_payload_file(name: &str) -> Result<File, Error> {
    if name.is_empty() || !name.is_ascii() || name.contains(['\\', ':', '~']) {
        return Err(Error::Untrusted);
    }
    for part in name.split('/') {
        let folded = part.to_ascii_lowercase();
        if part.is_empty()
            || matches!(part, "." | "..")
            || part.ends_with(['.', ' '])
            || part.bytes().any(|b| b < 32 || b"<>\"|?*".contains(&b))
            || matches!(
                folded.as_str(),
                "private" | "secrets" | "inventory" | "config" | "logs" | "userdata"
            )
            || folded.starts_with(".env")
            || [
                ".pem", ".key", ".pfx", ".p12", ".toml", ".log", ".ini", ".config", ".db",
                ".sqlite", ".sqlite3",
            ]
            .iter()
            .any(|ext| folded.ends_with(ext))
        {
            return Err(Error::Untrusted);
        }
        let base = folded.split('.').next().ok_or(Error::Untrusted)?;
        if matches!(
            base,
            "con" | "prn" | "aux" | "nul" | "clock$" | "conin$" | "conout$"
        ) || (base.len() == 4
            && (base.starts_with("com") || base.starts_with("lpt"))
            && matches!(base.as_bytes()[3], b'1'..=b'9'))
        {
            return Err(Error::Untrusted);
        }
    }
    let path = desk_installation()?.join(name);
    let user = User::current()?;
    let _parents = directory(
        path.parent().ok_or(Error::Untrusted)?,
        &user,
        Product::CustomerDesk,
    )?;
    let file = open(&path, false, false, false)?;
    inspect(&file, &user, Product::CustomerDesk, false, false)?;
    Ok(file)
}
