//! Windows Desk integration. Trust comes from compiled policy and LocalSystem,
//! never from GUI/config/argv. The independent guardian survives MSI service stop.
use super::{
    manifest::{Channel, Platform, Product},
    protected_store::{self, InstallationIdentity, ProtectedStore, StageOutcome},
    session_gate::{
        self,
        store_handles::{Child, ProtectedRoot},
    },
    LastAcceptedSequence,
};
use hbb_common::tokio;
use sha2::{Digest, Sha256};
use std::os::windows::process::CommandExt;
use std::{
    fs::File,
    io::{Read, Write},
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[derive(Debug)]
pub(crate) enum Error {
    Disabled,
    Policy,
    Trust,
    Store,
    Health,
    Installer,
    Io,
}
impl From<session_gate::Error> for Error {
    fn from(_: session_gate::Error) -> Self {
        Self::Trust
    }
}
impl From<protected_store::Error> for Error {
    fn from(_: protected_store::Error) -> Self {
        Self::Store
    }
}

struct Policy {
    key: [u8; 32],
    sequence: u64,
    source: String,
    origin: String,
    prefix: String,
}
impl Policy {
    fn compiled() -> Result<Self, Error> {
        if option_env!("ONGROW_WINDOWS_UPDATER") != Some("v1") {
            return Err(Error::Disabled);
        }
        if option_env!("ONGROW_PRODUCT_ROLE") != Some("customer-desk")
            || option_env!("ONGROW_UPDATE_SESSION_GATE") != Some("v1")
        {
            return Err(Error::Policy);
        }
        let sequence = option_env!("ONGROW_RELEASE_SEQUENCE")
            .ok_or(Error::Policy)?
            .parse()
            .map_err(|_| Error::Policy)?;
        let source = option_env!("ONGROW_RELEASE_SOURCE_SHA").ok_or(Error::Policy)?;
        let public = option_env!("ONGROW_UPDATE_PUBLIC_KEY_HEX").ok_or(Error::Policy)?;
        if !(1..=u32::MAX as u64).contains(&sequence)
            || source.len() != 40
            || !source
                .bytes()
                .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
            || public.len() != 64
        {
            return Err(Error::Policy);
        }
        let mut key = [0; 32];
        for (slot, pair) in key.iter_mut().zip(public.as_bytes().chunks_exact(2)) {
            let text = std::str::from_utf8(pair).map_err(|_| Error::Policy)?;
            *slot = u8::from_str_radix(text, 16).map_err(|_| Error::Policy)?;
        }
        let origin = option_env!("ONGROW_UPDATE_ORIGIN")
            .ok_or(Error::Policy)?
            .to_owned();
        let prefix = option_env!("ONGROW_UPDATE_PREFIX")
            .ok_or(Error::Policy)?
            .to_owned();
        let policy = Self {
            key,
            sequence,
            source: source.to_owned(),
            origin,
            prefix,
        };
        policy.identity()?;
        Ok(policy)
    }
    fn identity(&self) -> Result<InstallationIdentity, Error> {
        Ok(InstallationIdentity::trusted(
            Product::CustomerDesk,
            Platform::WindowsX64,
            Channel::Lab,
            self.sequence,
            &self.key,
            &self.origin,
            &self.prefix,
        )?)
    }
}
fn isolated() -> bool {
    ["ONGROW_CI_SMOKE_TEST", "GITHUB_ACTIONS"]
        .iter()
        .any(|name| matches!(std::env::var(name).ok().as_deref(), Some("1" | "true")))
}

// Public SDK ABI, avoiding new dependencies in the desktop crate. Values here
// are service/process access constants, never device identifiers.
#[repr(C)]
#[derive(Default)]
struct ServiceStatus {
    kind: u32,
    state: u32,
    controls: u32,
    exit: u32,
    specific: u32,
    checkpoint: u32,
    wait: u32,
    pid: u32,
    flags: u32,
}
type Handle = *mut std::ffi::c_void;
#[link(name = "advapi32")]
extern "system" {
    fn OpenSCManagerW(machine: *const u16, database: *const u16, access: u32) -> Handle;
    fn OpenServiceW(manager: Handle, name: *const u16, access: u32) -> Handle;
    fn CloseServiceHandle(handle: Handle) -> i32;
    fn QueryServiceStatusEx(
        service: Handle,
        level: u32,
        buffer: *mut u8,
        size: u32,
        needed: *mut u32,
    ) -> i32;
}
#[link(name = "kernel32")]
extern "system" {
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
    fn CloseHandle(handle: Handle) -> i32;
    fn GetProcessTimes(
        handle: Handle,
        creation: *mut u64,
        exit: *mut u64,
        kernel: *mut u64,
        user: *mut u64,
    ) -> i32;
    fn QueryFullProcessImageNameW(
        handle: Handle,
        flags: u32,
        buffer: *mut u16,
        length: *mut u32,
    ) -> i32;
    fn GetExitCodeProcess(handle: Handle, code: *mut u32) -> i32;
}
struct ServiceHandle(Handle);
impl Drop for ServiceHandle {
    fn drop(&mut self) {
        unsafe {
            CloseServiceHandle(self.0);
        }
    }
}
struct ProcessHandle(Handle);
impl Drop for ProcessHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
fn service_pid() -> Result<u32, Error> {
    let manager = ServiceHandle(unsafe { OpenSCManagerW(std::ptr::null(), std::ptr::null(), 1) });
    if manager.0.is_null() {
        return Err(Error::Health);
    }
    let service =
        ServiceHandle(unsafe { OpenServiceW(manager.0, wide("OnGROW Support Desk").as_ptr(), 4) });
    if service.0.is_null() {
        return Err(Error::Health);
    }
    let mut status = ServiceStatus::default();
    let mut needed = 0;
    if unsafe {
        QueryServiceStatusEx(
            service.0,
            0,
            (&mut status as *mut ServiceStatus).cast(),
            std::mem::size_of::<ServiceStatus>() as u32,
            &mut needed,
        )
    } == 0
        || status.state != 4
        || status.pid == 0
    {
        return Err(Error::Health);
    }
    Ok(status.pid)
}
fn process(pid: u32) -> Result<(ProcessHandle, u64, PathBuf), Error> {
    let handle = ProcessHandle(unsafe { OpenProcess(0x1000, 0, pid) });
    if handle.0.is_null() {
        return Err(Error::Health);
    }
    let (mut creation, mut exit, mut kernel, mut user) = (0, 0, 0, 0);
    if unsafe { GetProcessTimes(handle.0, &mut creation, &mut exit, &mut kernel, &mut user) } == 0 {
        return Err(Error::Health);
    }
    let mut buffer = [0u16; 32768];
    let mut length = buffer.len() as u32;
    if unsafe { QueryFullProcessImageNameW(handle.0, 0, buffer.as_mut_ptr(), &mut length) } == 0 {
        return Err(Error::Health);
    }
    let path = String::from_utf16(&buffer[..length as usize]).map_err(|_| Error::Health)?;
    Ok((handle, creation, PathBuf::from(path)))
}
fn equal_path(a: &std::path::Path, b: &std::path::Path) -> bool {
    a.as_os_str()
        .to_string_lossy()
        .eq_ignore_ascii_case(&b.as_os_str().to_string_lossy())
}
fn hash(mut file: &File) -> Result<[u8; 32], Error> {
    let mut digest = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let n = file.read(&mut buffer).map_err(|_| Error::Io)?;
        if n == 0 {
            break;
        }
        digest.update(&buffer[..n]);
    }
    Ok(digest.finalize().into())
}
fn loaded_health(sequence: u64, source: &str, pid: u32, created: u64) -> Vec<u8> {
    let mut bytes = b"OGHLTH1\0".to_vec();
    bytes.extend(sequence.to_be_bytes());
    bytes.extend(source.as_bytes());
    bytes.extend(pid.to_be_bytes());
    bytes.extend(created.to_be_bytes());
    bytes
}

/// Constructible only after checking protected loaded-code health against the
/// SCM's actual running PID, creation time and executable, not an MSI exit code.
pub(crate) struct HealthyService {
    pub(super) sequence: u64,
    _process: ProcessHandle,
    _exe: File,
    _core: File,
}
impl HealthyService {
    fn verify(root: &ProtectedRoot, sequence: u64, source: &str) -> Result<Self, Error> {
        let pid = service_pid()?;
        let (process, created, path) = process(pid)?;
        if !equal_path(
            &path,
            &session_gate::windows_bootstrap::installation()?.join("OnGROW Support Desk.exe"),
        ) {
            return Err(Error::Health);
        }
        let exe = session_gate::windows_bootstrap::installed_file("OnGROW Support Desk.exe")?;
        let core = session_gate::windows_bootstrap::installed_file("librustdesk.dll")?;
        let mut marker = root.read(Child::Health)?;
        if marker.metadata().map_err(|_| Error::Health)?.len() != 68 {
            return Err(Error::Health);
        }
        let mut bytes = Vec::new();
        marker
            .take(69)
            .read_to_end(&mut bytes)
            .map_err(|_| Error::Health)?;
        if bytes != loaded_health(sequence, source, pid, created) || service_pid()? != pid {
            return Err(Error::Health);
        }
        let mut code = 0;
        if unsafe { GetExitCodeProcess(process.0, &mut code) } == 0 || code != 259 {
            return Err(Error::Health);
        }
        Ok(Self {
            sequence,
            _process: process,
            _exe: exe,
            _core: core,
        })
    }
}

/// Called by the actual service after its server child and IPC listener exist.
/// It never declares an update complete or clears Pending itself.
pub(crate) fn service_ready() -> Result<(), Error> {
    let policy = match Policy::compiled() {
        Err(Error::Disabled) => return Ok(()),
        result => result?,
    };
    session_gate::windows_bootstrap::system()?;
    let created = session_gate::windows_bootstrap::root()?;
    if created {
        ProtectedStore::bootstrap(policy.identity()?)?;
        session_gate::initialize()?;
    }
    let root = ProtectedRoot::open(session_gate::Product::CustomerDesk)?;
    let pid = std::process::id();
    let (_, created, path) = process(pid)?;
    if !equal_path(
        &path,
        &session_gate::windows_bootstrap::installation()?.join("OnGROW Support Desk.exe"),
    ) {
        return Err(Error::Trust);
    }
    root.replace(
        Child::Health,
        &loaded_health(policy.sequence, &policy.source, pid, created),
    )?;
    if isolated() {
        return Ok(());
    }
    tokio::spawn(async {
        tokio::time::sleep(Duration::from_secs(30)).await;
        loop {
            let result = tokio::task::spawn_blocking(run_guardian).await;
            let delay = match result {
                Ok(Ok(0 | 10)) => 86400,
                _ => 1800,
            };
            tokio::time::sleep(Duration::from_secs(delay)).await;
        }
    });
    Ok(())
}
fn run_guardian() -> Result<i32, Error> {
    let root = ProtectedRoot::open(session_gate::Product::CustomerDesk)?;
    let lease = root.stage_lease()?;
    // Do not replace a running guardian. Its image cannot be deleted on Windows.
    match root.read(Child::Guardian) {
        Ok(file) => {
            drop(file);
            root.remove(Child::Guardian)?;
        }
        Err(session_gate::Error::MissingGate) => {}
        Err(error) => return Err(error.into()),
    }
    let mut source = session_gate::windows_bootstrap::installed_file("ongrow-update.exe")?;
    let expected = hash(&source)?;
    use std::io::{Seek, SeekFrom};
    source.seek(SeekFrom::Start(0)).map_err(|_| Error::Io)?;
    let mut target = root.create(Child::Guardian)?;
    std::io::copy(&mut source, &mut target).map_err(|_| Error::Io)?;
    root.sync_file(&target)?;
    drop(target);
    if hash(&root.read(Child::Guardian)?)? != expected {
        return Err(Error::Trust);
    }
    let executable = root.path(Child::Guardian);
    drop(lease);
    let mut child = Command::new(executable)
        .arg("--check")
        .creation_flags(0x08000000)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| Error::Io)?;
    // This thread disappearing when MSI stops the service does not kill child.
    let exit = child.wait().map_err(|_| Error::Io)?;
    Ok(exit.code().unwrap_or(1))
}

/// Standalone guardian command: no arbitrary paths, commands or runtime keys.
/// A failed native start keeps Pending. There is no transaction kill on timeout.
pub(crate) async fn check_once() -> Result<i32, Error> {
    let policy = Policy::compiled()?;
    session_gate::windows_bootstrap::system()?;
    if isolated() {
        return Err(Error::Disabled);
    }
    let root = ProtectedRoot::open(session_gate::Product::CustomerDesk)?;
    let current = std::env::current_exe().map_err(|_| Error::Io)?;
    if !equal_path(&current, &root.path(Child::Guardian)) {
        return Err(Error::Trust);
    }
    let store = ProtectedStore::open(policy.identity()?)?;
    store.snapshot()?;
    let recovery = match session_gate::admit() {
        Ok(lease) => {
            drop(lease);
            None
        }
        Err(session_gate::Error::Pending) => Some(session_gate::Transaction::recover()?),
        Err(session_gate::Error::Busy) => return Ok(11),
        Err(error) => return Err(error.into()),
    };
    let mut ticket = if let Some(ref transaction) = recovery {
        match store.resume_stage_recovery(transaction).await.await {
            Ok(ticket) => ticket,
            Err(protected_store::Error::Trust(session_gate::Error::MissingGate)) => {
                // Crash after committed sequence/slot cleanup but before Ready.
                let accepted = store.snapshot()?.state();
                if !matches!(accepted,LastAcceptedSequence::Known(s) if s==policy.sequence) {
                    return Err(Error::Store);
                }
                let health = HealthyService::verify(&root, policy.sequence, &policy.source)?;
                store.cleanup_committed(&health)?;
                store.cleanup_committed(&health)?;
                recovery.ok_or(Error::Trust)?.finish_windows(health)?;
                return Ok(10);
            }
            Err(error) => return Err(error.into()),
        }
    } else {
        match store.resume_stage().await {
            Ok(ticket) => ticket,
            Err(protected_store::Error::Trust(session_gate::Error::MissingGate)) => {
                match store.download(true).await? {
                    StageOutcome::NoUpdate => return Ok(0),
                    StageOutcome::Sealed(ticket) => ticket,
                }
            }
            Err(error) => return Err(error.into()),
        }
    };
    let sequence = ticket.manifest().release_sequence;
    let source = ticket.manifest().source_sha.clone();
    let transaction = match recovery {
        Some(t) => t,
        None => match session_gate::begin_installation() {
            Ok(t) => t,
            Err(session_gate::Error::Busy) => return Ok(11),
            Err(error) => return Err(error.into()),
        },
    };
    ticket.reverify()?;
    // If a crash happened after native commit, authentic loaded health wins;
    // never repeat MSI merely because its previous caller disappeared.
    let health = match HealthyService::verify(&root, sequence, &source) {
        Ok(health) => health,
        Err(_) => {
            // The signed ticket remains held, denying write/replace of MSI bytes.
            let executable = session_gate::windows_bootstrap::system_executable("msiexec.exe")?;
            let payload = ticket.installer_path()?;
            let mut child = Command::new(executable)
                .arg("/i")
                .arg(payload)
                .args(["/qn", "/norestart", "REBOOT=ReallySuppress"])
                .creation_flags(0x08000000)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|_| Error::Installer)?;
            let started = Instant::now();
            let code = loop {
                match child.try_wait().map_err(|_| Error::Installer)? {
                    Some(exit) => break exit.code().ok_or(Error::Installer)?,
                    None => {}
                }
                if started.elapsed() >= Duration::from_secs(600) {
                    return Err(Error::Installer);
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            };
            if code != 0 && code != 3010 {
                return Err(Error::Installer);
            }
            let started = Instant::now();
            loop {
                if let Ok(health) = HealthyService::verify(&root, sequence, &source) {
                    break health;
                }
                if started.elapsed() >= Duration::from_secs(120) {
                    return Err(Error::Health);
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    };
    if health.sequence != sequence {
        return Err(Error::Health);
    }
    store.commit_windows(ticket, &health)?;
    transaction.finish_windows(health)?;
    Ok(10)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn loaded_marker_distinguishes_revision_pid_and_process_creation() {
        let first = loaded_health(10, &"a".repeat(40), 7, 100);
        assert_eq!(first.len(), 68);
        assert_ne!(first, loaded_health(11, &"a".repeat(40), 7, 100));
        assert_ne!(first, loaded_health(10, &"b".repeat(40), 7, 100));
        assert_ne!(first, loaded_health(10, &"a".repeat(40), 8, 100));
        assert_ne!(first, loaded_health(10, &"a".repeat(40), 7, 101));
    }
    #[test]
    fn principal_is_required_before_any_live_guardian_effect() {
        // GitHub's ordinary elevated runner is not LocalSystem. This rejects
        // before a state root, HTTP request, installer or deletion is possible.
        assert!(session_gate::windows_bootstrap::system().is_err());
    }
    #[test]
    fn a_file_or_unregistered_process_does_not_prove_service_health() {
        assert!(service_pid().is_err());
    }
}
