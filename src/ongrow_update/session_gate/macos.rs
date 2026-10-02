use super::{Error, Product};
use hbb_common::libc;
use std::{
    ffi::{CStr, CString},
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    os::unix::{ffi::OsStrExt, fs::MetadataExt, io::{AsRawFd, FromRawFd}},
    path::{Component, Path, PathBuf},
};

const GATE: &str = "admission-v1.lock";
const JOURNAL: &str = "state-v1.journal";
const READY: u8 = 0;
const PENDING: u8 = 1;

// Apple's extended ACL API is not exposed by the existing libc crate.
// These declarations mirror the SDK's sys/acl.h and use no new dependency.
extern "C" {
    fn acl_get_fd_np(fd: libc::c_int, acl_type: libc::c_int) -> *mut libc::c_void;
    fn acl_valid(acl: *mut libc::c_void) -> libc::c_int;
    fn acl_get_entry(acl: *mut libc::c_void, entry: libc::c_int, out: *mut *mut libc::c_void) -> libc::c_int;
    fn acl_get_tag_type(entry: *mut libc::c_void, tag: *mut libc::c_int) -> libc::c_int;
    fn acl_get_permset_mask_np(entry: *mut libc::c_void, mask: *mut u64) -> libc::c_int;
    fn acl_free(acl: *mut libc::c_void) -> libc::c_int;
}
struct Acl(*mut libc::c_void);
impl Drop for Acl {
    fn drop(&mut self) { if !self.0.is_null() { unsafe { acl_free(self.0); } } }
}

fn open_at(parent: &File, name: &str, flags: i32, mode: u16) -> Result<File, Error> {
    let name = CString::new(name).map_err(|_| Error::Untrusted)?;
    let fd = unsafe {
        libc::openat(parent.as_raw_fd(), name.as_ptr(), flags | libc::O_CLOEXEC | libc::O_NOFOLLOW, mode as libc::c_uint)
    };
    if fd < 0 {
        return Err(if std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT) { Error::MissingGate } else { Error::Untrusted });
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn inspect_metadata(file: &File, owner: u32, directory: bool, ancestor: bool) -> Result<(), Error> {
    let metadata = file.metadata().map_err(|_| Error::Untrusted)?;
    if (directory && !metadata.is_dir()) || (!directory && (!metadata.is_file() || metadata.nlink() != 1))
        || (metadata.uid() != owner && !(ancestor && metadata.uid() == 0))
        || metadata.mode() & 0o022 != 0
    { return Err(Error::Untrusted); }
    let mut fs: libc::statfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstatfs(file.as_raw_fd(), &mut fs) } != 0 || fs.f_flags & libc::MNT_LOCAL as u32 == 0 {
        return Err(Error::Untrusted);
    }
    Ok(())
}
fn inspect(file: &File, owner: u32, directory: bool, ancestor: bool) -> Result<(), Error> {
    inspect_metadata(file, owner, directory, ancestor)?;
    // ACL_TYPE_EXTENDED from Apple's sys/acl.h. Apple libc reports ENOENT when
    // an existing file has no extended ACL. Capture errno before any other call
    // and revalidate the same handle; no other retrieval failure is accepted.
    let pointer = unsafe { acl_get_fd_np(file.as_raw_fd(), 0x100) };
    let acl_error = std::io::Error::last_os_error().raw_os_error();
    let acl = Acl(pointer);
    if acl.0.is_null() && acl_error == Some(libc::ENOENT) {
        return inspect_metadata(file, owner, directory, ancestor);
    }
    if acl.0.is_null() || unsafe { acl_valid(acl.0) } != 0 { return Err(Error::Untrusted); }
    let mut entry = std::ptr::null_mut();
    let mut which = 0; // ACL_FIRST_ENTRY
    loop {
        let result = unsafe { acl_get_entry(acl.0, which, &mut entry) };
        // Darwin returns 0 for an entry and -1/EINVAL at the end. Validate the
        // kernel-provided ACL first, so malformed ACLs cannot masquerade as end.
        if result == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::EINVAL) { break; }
        if result != 0 || entry.is_null() { return Err(Error::Untrusted); }
        which = -1; // ACL_NEXT_ENTRY
        let mut tag = 0;
        let mut permissions = 0;
        if unsafe { acl_get_tag_type(entry, &mut tag) } != 0
            || unsafe { acl_get_permset_mask_np(entry, &mut permissions) } != 0
        { return Err(Error::Untrusted); }
        // Deny ACLs are safe. Reject every allow ACE capable of mutating an
        // object, even for its owner. This deliberately errs on the safe side.
        const MUTATION: u64 = (1 << 2) | (1 << 4) | (1 << 5) | (1 << 6) | (1 << 8) | (1 << 10) | (1 << 12) | (1 << 13);
        if tag == 1 && permissions & MUTATION != 0 { return Err(Error::Untrusted); }
        if tag != 1 && tag != 2 { return Err(Error::Untrusted); }
    }
    Ok(())
}

fn trusted_directory(path: &Path, owner: u32) -> Result<File, Error> {
    if !path.is_absolute() { return Err(Error::Untrusted); }
    let root = CString::new("/").map_err(|_| Error::Untrusted)?;
    let fd = unsafe { libc::open(root.as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW) };
    if fd < 0 { return Err(Error::Untrusted); }
    let mut directory = unsafe { File::from_raw_fd(fd) };
    inspect(&directory, owner, true, true)?;
    let components: Vec<_> = path.components().collect();
    for (index, component) in components.iter().enumerate() {
        match component {
            Component::RootDir => continue,
            Component::Normal(name) => {
                let name = name.to_str().ok_or(Error::Untrusted)?;
                directory = open_at(&directory, name, libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
                inspect(&directory, owner, true, index + 1 < components.len())?;
            }
            _ => return Err(Error::Untrusted),
        }
    }
    Ok(directory)
}

fn root(product: Product) -> Result<(PathBuf, u32), Error> {
    match product {
        Product::CustomerDesk => Ok((PathBuf::from("/Library/Application Support/OnGROW/Support Desk Update"), 0)),
        Product::SupportConsole => {
            let uid = unsafe { libc::geteuid() };
            if uid == 0 { return Err(Error::Untrusted); }
            let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
            let mut result = std::ptr::null_mut();
            let mut buffer = vec![0u8; 64 * 1024];
            if unsafe { libc::getpwuid_r(uid, &mut entry, buffer.as_mut_ptr().cast(), buffer.len(), &mut result) } != 0
                || result.is_null() || entry.pw_dir.is_null() || entry.pw_uid != uid
            { return Err(Error::Untrusted); }
            let home = unsafe { CStr::from_ptr(entry.pw_dir) }.to_bytes();
            let path = Path::new(std::ffi::OsStr::from_bytes(home)).join("Library/Application Support/OnGROW/Support Console Update");
            Ok((path, uid))
        }
    }
}

pub(super) struct Lock {
    gate: File,
    journal: File,
    _directory: File,
    exclusive: bool,
}
impl Drop for Lock {
    fn drop(&mut self) { unsafe { libc::flock(self.gate.as_raw_fd(), libc::LOCK_UN); } }
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
        // A fixed one-byte journal needs no rename or removable inode. Readers
        // cannot observe the change while the writer owns the exclusive gate.
        // No process may stop until the full flush has completed successfully.
        self.journal.seek(SeekFrom::Start(0)).map_err(|_| Error::Io)?;
        self.journal.write_all(&[state]).map_err(|_| Error::Io)?;
        self.journal.sync_all().map_err(|_| Error::Io)?;
        if unsafe { libc::fcntl(self.journal.as_raw_fd(), libc::F_FULLFSYNC) } != 0 { return Err(Error::Io); }
        Ok(())
    }
    pub(super) fn mark_pending(&mut self) -> Result<(), Error> {
        if self.state()? == PENDING { return Err(Error::Pending); }
        self.write_state(PENDING)
    }
    pub(super) fn mark_ready(&mut self) -> Result<(), Error> { self.write_state(READY) }
}

fn acquire(path: &Path, owner: u32, exclusive: bool) -> Result<Lock, Error> {
    let directory = trusted_directory(path, owner)?;
    let access = if exclusive { libc::O_RDWR } else { libc::O_RDONLY };
    let gate = open_at(&directory, GATE, access, 0)?;
    inspect(&gate, owner, false, false)?;
    if gate.metadata().map_err(|_| Error::Untrusted)?.len() != 0 { return Err(Error::Untrusted); }
    let flag = if exclusive { libc::LOCK_EX } else { libc::LOCK_SH };
    if unsafe { libc::flock(gate.as_raw_fd(), flag | libc::LOCK_NB) } != 0 {
        return Err(if std::io::Error::last_os_error().raw_os_error() == Some(libc::EWOULDBLOCK) { Error::Busy } else { Error::Io });
    }
    let journal = open_at(&directory, JOURNAL, access, 0)?;
    inspect(&journal, owner, false, false)?;
    let mut lock = Lock { gate, journal, _directory: directory, exclusive };
    if lock.state()? == PENDING && !exclusive { return Err(Error::Pending); }
    Ok(lock)
}

pub(super) fn admit(product: Product) -> Result<Lock, Error> {
    let (path, owner) = root(product)?;
    acquire(&path, owner, false)
}
pub(super) fn exclusive(product: Product) -> Result<Lock, Error> {
    let (path, owner) = root(product)?;
    acquire(&path, owner, true)
}
pub(super) fn initialize(product: Product) -> Result<(), Error> {
    let (path, owner) = root(product)?;
    initialize_at(&path, owner)
}
fn initialize_at(path: &Path, owner: u32) -> Result<(), Error> {
    // Creating and granting the protected directory belongs to bootstrap.
    // Never reset an existing journal, even if only one file exists.
    let directory = trusted_directory(path, owner)?;
    for name in [GATE, JOURNAL] {
        let name = CString::new(name).map_err(|_| Error::Untrusted)?;
        let mut metadata: libc::stat = unsafe { std::mem::zeroed() };
        if unsafe { libc::fstatat(directory.as_raw_fd(), name.as_ptr(), &mut metadata, libc::AT_SYMLINK_NOFOLLOW) } == 0
            || std::io::Error::last_os_error().raw_os_error() != Some(libc::ENOENT)
        { return Err(Error::Untrusted); }
    }
    let mut journal = open_at(&directory, JOURNAL, libc::O_RDWR | libc::O_CREAT | libc::O_EXCL, 0o644)?;
    inspect(&journal, owner, false, false)?;
    journal.write_all(&[READY]).map_err(|_| Error::Io)?;
    journal.sync_all().map_err(|_| Error::Io)?;
    if unsafe { libc::fcntl(journal.as_raw_fd(), libc::F_FULLFSYNC) } != 0 { return Err(Error::Io); }
    let gate = open_at(&directory, GATE, libc::O_RDWR | libc::O_CREAT | libc::O_EXCL, 0o644)?;
    inspect(&gate, owner, false, false)?;
    gate.sync_all().map_err(|_| Error::Io)?;
    directory.sync_all().map_err(|_| Error::Io)?;
    Ok(())
}

#[cfg(test)]
pub(super) fn fixture_initialize(path: &Path) -> Result<(), Error> {
    initialize_at(path, unsafe { libc::geteuid() })
}
#[cfg(test)]
pub(super) fn fixture_acquire(path: &Path, exclusive: bool) -> Result<Lock, Error> {
    acquire(path, unsafe { libc::geteuid() }, exclusive)
}
