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
            || option_env!("ONGROW_MSI_UPGRADE_CODE").map_or(true, |code| code.len() != 36)
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
        verify_installed_payload(sequence, source)?;
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

/// Called by the actual service after its IPC listener exists.
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
    drop(source);
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
        match store.resume_stage_recovery(transaction).await {
            Ok(ticket) => ticket,
            Err(protected_store::Error::Trust(session_gate::Error::MissingGate)) => {
                // Crash after committed sequence/slot cleanup but before Ready.
                let accepted = store.snapshot()?.state();
                if !matches!(accepted,LastAcceptedSequence::Known(s) if s==policy.sequence) {
                    return Err(Error::Store);
                }
                match root.read(Child::NativeState) {
                    Ok(file) => {
                        let record = read_native_record(file)?;
                        let code = u32::from_be_bytes(
                            record[124..128].try_into().map_err(|_| Error::Installer)?,
                        );
                        let target = u64::from_be_bytes(
                            record[8..16].try_into().map_err(|_| Error::Installer)?,
                        );
                        if record[120] != 2
                            || code == u32::MAX
                            || ((code == 0 || code == 3010)
                                && (target != policy.sequence
                                    || record[16..56] != *policy.source.as_bytes()))
                        {
                            return Err(Error::Installer);
                        }
                    }
                    Err(session_gate::Error::MissingGate) => {}
                    Err(error) => return Err(error.into()),
                }
                let health = HealthyService::verify(&root, policy.sequence, &policy.source)?;
                store.cleanup_committed(&health)?;
                recovery.ok_or(Error::Trust)?.finish_windows(health)?;
                return Ok(10);
            }
            Err(error) => return Err(error.into()),
        }
    } else {
        match store.resume_stage().await {
            Ok(ticket) => ticket,
            Err(protected_store::Error::Trust(session_gate::Error::Busy)) => return Ok(11),
            Err(protected_store::Error::Trust(session_gate::Error::Untrusted)) => {
                return Err(Error::Trust)
            }
            Err(_) => {
                // A failed download or expired pre-install attempt must not
                // occupy the sole slot forever. Pending can never enter here.
                let ready = session_gate::admit()?;
                store.discard_ready(&ready)?;
                drop(ready);
                match store.download(true).await? {
                    StageOutcome::NoUpdate => return Ok(0),
                    StageOutcome::Sealed(ticket) => ticket,
                }
            }
        }
    };
    let sequence = ticket.manifest().release_sequence;
    let source = ticket.manifest().source_sha.clone();
    if recovery.is_none() {
        let validation = ticket
            .installer_path()
            .map_err(Error::from)
            .and_then(|path| validate_msi(&path, sequence));
        if let Err(error) = validation {
            let ready = session_gate::admit()?;
            drop(ticket);
            store.discard_ready(&ready)?;
            return Err(error);
        }
    }
    let transaction = match recovery {
        Some(t) => t,
        None => match session_gate::begin_installation() {
            Ok(t) => t,
            Err(session_gate::Error::Busy) => return Ok(11),
            Err(error) => return Err(error.into()),
        },
    };
    ticket.reverify()?;
    // A surviving native installer is not quiescent just because the guardian
    // died or a new service already started. Only durable caller completion
    // permits recovery to commit health; Started/partial records stay Pending.
    let native = native_result(&root, ticket.manifest())?;
    let code = match native {
        Some(code) => code,
        None => {
            let executable = session_gate::windows_bootstrap::system_executable("msiexec.exe")?;
            let payload = ticket.installer_path()?;
            validate_msi(&payload, sequence)?;
            quiesce_apps().await?;
            let mut started = root.create(Child::NativeState)?;
            started
                .write_all(&native_record(ticket.manifest(), None))
                .map_err(|_| Error::Io)?;
            root.sync_file(&started)?;
            drop(started);
            let launch = Command::new(executable)
                .arg("/i")
                .arg(payload)
                .args(["/qn", "/norestart", "REBOOT=ReallySuppress"])
                .creation_flags(0x08000000)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
            let mut child = match launch {
                Ok(child) => child,
                Err(_) => {
                    // No child exists on a failed CreateProcess. Record this
                    // known non-start so authentic previous health may recover.
                    root.replace(
                        Child::NativeState,
                        &native_record(ticket.manifest(), Some(1603)),
                    )?;
                    return Err(Error::Installer);
                }
            };
            let began = Instant::now();
            let code = loop {
                match child.try_wait().map_err(|_| Error::Installer)? {
                    Some(exit) => {
                        break u32::try_from(exit.code().ok_or(Error::Installer)?)
                            .map_err(|_| Error::Installer)?
                    }
                    None => {}
                }
                if began.elapsed() >= Duration::from_secs(600) {
                    return Err(Error::Installer);
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            };
            root.replace(
                Child::NativeState,
                &native_record(ticket.manifest(), Some(code)),
            )?;
            code
        }
    };
    let started = Instant::now();
    if code != 0 && code != 3010 {
        loop {
            if let Ok(previous) = HealthyService::verify(&root, policy.sequence, &policy.source) {
                store.rollback_windows(ticket, &previous)?;
                transaction.finish_windows(previous)?;
                return Ok(1);
            }
            if started.elapsed() >= Duration::from_secs(120) {
                return Err(Error::Installer);
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
    let health = loop {
        if let Ok(health) = HealthyService::verify(&root, sequence, &source) {
            break health;
        }
        if started.elapsed() >= Duration::from_secs(120) {
            return Err(Error::Health);
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
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
    #[test]
    fn payload_health_cannot_read_private_paths_or_windows_aliases() {
        for name in [
            "../private/key",
            "data/.env",
            "data/config/settings.json",
            "private/password.txt",
            "secrets/key.json",
            "data/secret.pem",
            "data/foo:bar",
            "data/CON.txt",
            "data/private~1/key",
        ] {
            assert!(session_gate::windows_bootstrap::payload_file(name).is_err());
        }
    }
}

/// Shared protected journal observation. An error never requests process exit.
pub(crate) fn apply_pending() -> bool {
    session_gate::windows_bootstrap::pending() == Ok(true)
}
#[derive(Clone, Copy)]
pub(crate) enum Restart {
    Gui,
    Tray,
}

/// The original app closes itself in its own Windows session. SYSTEM does not
/// impersonate users, broadcast window messages or kill other principals.
pub(crate) fn start_process_watcher(restart: Option<Restart>, quit: fn()) -> Result<(), Error> {
    match Policy::compiled() {
        Err(Error::Disabled) => return Ok(()),
        result => {
            result?;
        }
    }
    let expected = session_gate::windows_bootstrap::installation()?.join("OnGROW Support Desk.exe");
    if !equal_path(&std::env::current_exe().map_err(|_| Error::Io)?, &expected) {
        return Err(Error::Trust);
    }
    std::thread::Builder::new()
        .name("ongrow-update-admission".into())
        .spawn(move || loop {
            if apply_pending() {
                if let Some(mode) = restart {
                    let launch = || -> Result<(), Error> {
                        let root = ProtectedRoot::open(session_gate::Product::CustomerDesk)?;
                        let _image = root.read(Child::Guardian)?;
                        Command::new(root.path(Child::Guardian))
                            .arg(match mode {
                                Restart::Gui => "--wait-restart",
                                Restart::Tray => "--wait-restart-tray",
                            })
                            .creation_flags(0x08000000)
                            .stdin(Stdio::null())
                            .stdout(Stdio::null())
                            .stderr(Stdio::null())
                            .spawn()
                            .map_err(|_| Error::Io)?;
                        Ok(())
                    };
                    if launch().is_err() {
                        std::thread::sleep(Duration::from_secs(1));
                        continue;
                    }
                }
                quit();
                return;
            }
            std::thread::sleep(Duration::from_secs(1));
        })
        .map_err(|_| Error::Io)?;
    Ok(())
}

/// A user child of the original GUI waits outside the replacement directory.
/// It has no installer/state-write authority and launches only the fixed app.
pub(crate) async fn wait_restart(tray: bool) -> Result<i32, Error> {
    Policy::compiled()?;
    let root = ProtectedRoot::open(session_gate::Product::CustomerDesk)?;
    if !equal_path(
        &std::env::current_exe().map_err(|_| Error::Io)?,
        &root.path(Child::Guardian),
    ) {
        return Err(Error::Trust);
    }
    let started = Instant::now();
    loop {
        if !session_gate::windows_bootstrap::pending()? {
            let lease = match session_gate::admit() {
                Ok(lease) => lease,
                Err(session_gate::Error::Busy | session_gate::Error::Pending) => {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
                Err(error) => return Err(error.into()),
            };
            let executable =
                session_gate::windows_bootstrap::installation()?.join("OnGROW Support Desk.exe");
            let _trusted =
                session_gate::windows_bootstrap::installed_file("OnGROW Support Desk.exe")?;
            let mut command = Command::new(executable);
            if tray {
                command.arg("--tray");
            }
            command
                .creation_flags(0x08000000)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|_| Error::Io)?;
            drop(lease);
            return Ok(0);
        }
        if started.elapsed() >= Duration::from_secs(900) {
            return Err(Error::Health);
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn quiesce_apps() -> Result<(), Error> {
    use windows::Win32::{
        Foundation::{CloseHandle as CloseSnapshot, INVALID_HANDLE_VALUE},
        System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        },
    };
    let snapshot =
        unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }.map_err(|_| Error::Health)?;
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(Error::Health);
    }
    struct Snapshot(windows::Win32::Foundation::HANDLE);
    impl Drop for Snapshot {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseSnapshot(self.0);
            }
        }
    }
    let snapshot = Snapshot(snapshot);
    let mut entry = PROCESSENTRY32W::default();
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
    unsafe { Process32FirstW(snapshot.0, &mut entry) }.map_err(|_| Error::Health)?;
    let mut valid = true;
    let service = service_pid().ok();
    let expected = session_gate::windows_bootstrap::installation()?.join("OnGROW Support Desk.exe");
    let mut processes = Vec::new();
    while valid {
        let length = entry
            .szExeFile
            .iter()
            .position(|c| *c == 0)
            .unwrap_or(entry.szExeFile.len());
        let name = String::from_utf16(&entry.szExeFile[..length]).map_err(|_| Error::Health)?;
        if name.eq_ignore_ascii_case("OnGROW Support Desk.exe")
            && Some(entry.th32ProcessID) != service
        {
            let (handle, _, path) = process(entry.th32ProcessID)?;
            if equal_path(&path, &expected) {
                processes.push(handle);
            }
        }
        valid = match unsafe { Process32NextW(snapshot.0, &mut entry) } {
            Ok(()) => true,
            Err(error) if error.code().0 as u32 == 0x80070012 => false,
            Err(_) => return Err(Error::Health),
        };
    }
    let started = Instant::now();
    loop {
        let mut all_stopped = true;
        for process in &processes {
            let mut code = 0;
            if unsafe { GetExitCodeProcess(process.0, &mut code) } == 0 {
                return Err(Error::Health);
            }
            if code == 259 {
                all_stopped = false;
            }
        }
        if all_stopped {
            return Ok(());
        }
        if started.elapsed() >= Duration::from_secs(30) {
            return Err(Error::Health);
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

// Read-only native MSI properties before any native installer starts. The signed
// payload itself still remains pinned read-only throughout the transaction.
#[link(name = "msi")]
extern "system" {
    fn MsiOpenDatabaseW(path: *const u16, persist: *const u16, database: *mut u32) -> u32;
    fn MsiDatabaseOpenViewW(database: u32, query: *const u16, view: *mut u32) -> u32;
    fn MsiViewExecute(view: u32, record: u32) -> u32;
    fn MsiViewFetch(view: u32, record: *mut u32) -> u32;
    fn MsiRecordGetStringW(record: u32, field: u32, buffer: *mut u16, length: *mut u32) -> u32;
    fn MsiCloseHandle(handle: u32) -> u32;
}
struct MsiHandle(u32);
impl Drop for MsiHandle {
    fn drop(&mut self) {
        unsafe {
            MsiCloseHandle(self.0);
        }
    }
}
fn msi_property(database: u32, name: &str) -> Result<String, Error> {
    if !matches!(
        name,
        "ProductName" | "Manufacturer" | "ProductVersion" | "UpgradeCode" | "ALLUSERS"
    ) {
        return Err(Error::Trust);
    }
    let query = wide(&format!(
        "SELECT `Value` FROM `Property` WHERE `Property` = '{name}'"
    ));
    let mut view = MsiHandle(0);
    if unsafe { MsiDatabaseOpenViewW(database, query.as_ptr(), &mut view.0) } != 0
        || unsafe { MsiViewExecute(view.0, 0) } != 0
    {
        return Err(Error::Installer);
    }
    let mut record = MsiHandle(0);
    if unsafe { MsiViewFetch(view.0, &mut record.0) } != 0 {
        return Err(Error::Installer);
    }
    let mut buffer = [0u16; 1024];
    let mut length = buffer.len() as u32;
    if unsafe { MsiRecordGetStringW(record.0, 1, buffer.as_mut_ptr(), &mut length) } != 0
        || length >= buffer.len() as u32
    {
        return Err(Error::Installer);
    }
    String::from_utf16(&buffer[..length as usize]).map_err(|_| Error::Installer)
}
fn validate_msi(path: &std::path::Path, sequence: u64) -> Result<(), Error> {
    use std::os::windows::ffi::OsStrExt;
    let mut database = MsiHandle(0);
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    if unsafe { MsiOpenDatabaseW(path.as_ptr(), std::ptr::null(), &mut database.0) } != 0 {
        return Err(Error::Installer);
    }
    let version = format!(
        "{}.{}.{}",
        sequence >> 24,
        (sequence >> 16) & 255,
        sequence & 65535
    );
    let upgrade = option_env!("ONGROW_MSI_UPGRADE_CODE").ok_or(Error::Policy)?;
    if msi_property(database.0, "ProductName")? != "OnGROW Support Desk"
        || msi_property(database.0, "Manufacturer")? != "OnGROW GmbH"
        || msi_property(database.0, "ProductVersion")? != version
        || msi_property(database.0, "ALLUSERS")? != "1"
        || msi_property(database.0, "UpgradeCode")?.trim_matches(['{', '}']) != upgrade
    {
        return Err(Error::Installer);
    }
    Ok(())
}

#[derive(serde_derive::Deserialize)]
#[serde(deny_unknown_fields)]
struct InstalledPayload {
    product: String,
    sequence: u64,
    source_sha: String,
    sha256: std::collections::BTreeMap<String, String>,
}
fn verify_installed_payload(sequence: u64, source: &str) -> Result<(), Error> {
    let file = session_gate::windows_bootstrap::payload_file("ongrow-installed-payload.json")?;
    if file.metadata().map_err(|_| Error::Health)?.len() > 1024 * 1024 {
        return Err(Error::Health);
    }
    let mut raw = Vec::new();
    file.take(1024 * 1024 + 1)
        .read_to_end(&mut raw)
        .map_err(|_| Error::Health)?;
    if raw.len() > 1024 * 1024 {
        return Err(Error::Health);
    }
    let manifest: InstalledPayload = serde_json::from_slice(&raw).map_err(|_| Error::Health)?;
    if manifest.product != "customer-desk"
        || manifest.sequence != sequence
        || manifest.source_sha != source
        || manifest.sha256.is_empty()
        || manifest.sha256.len() > 4096
    {
        return Err(Error::Health);
    }
    for required in [
        "OnGROW Support Desk.exe",
        "librustdesk.dll",
        "flutter_windows.dll",
        "ongrow-update.exe",
        "data/icudtl.dat",
        "data/flutter_assets/AssetManifest.json",
    ] {
        if !manifest.sha256.contains_key(required) {
            return Err(Error::Health);
        }
    }
    let mut aliases = std::collections::BTreeSet::new();
    for (name, expected) in manifest.sha256 {
        if !aliases.insert(name.to_ascii_lowercase())
            || name == "ongrow-installed-payload.json"
            || expected.len() != 64
            || !expected
                .bytes()
                .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
        {
            return Err(Error::Health);
        }
        let file = session_gate::windows_bootstrap::payload_file(&name)?;
        let actual = hash(&file)?
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        if actual != expected {
            return Err(Error::Health);
        }
    }
    Ok(())
}

#[cfg(test)]
mod system_probe {
    use super::*;
    #[test]
    #[ignore = "only the explicit disposable LocalSystem CI action may run this"]
    fn protected_desk_bootstrap_staging_and_pending() {
        assert_eq!(
            std::env::var("ONGROW_WINDOWS_SYSTEM_PROBE").as_deref(),
            Ok("approved-disposable-runner")
        );
        eprintln!("SYSTEM_PROBE_PHASE:SYSTEM");
        session_gate::windows_bootstrap::system().unwrap();
        eprintln!("SYSTEM_PROBE_PHASE:ROOT");
        assert!(
            session_gate::windows_bootstrap::root().unwrap(),
            "Fixture must be a fresh root"
        );
        let identity = InstallationIdentity::trusted(
            Product::CustomerDesk,
            Platform::WindowsX64,
            Channel::Lab,
            1,
            &[0xab; 32],
            "https://updates.example.com",
            "/lab/",
        )
        .unwrap();
        eprintln!("SYSTEM_PROBE_PHASE:STORE");
        let store = ProtectedStore::bootstrap(identity).unwrap();
        eprintln!("SYSTEM_PROBE_PHASE:GATE");
        session_gate::initialize().unwrap();
        let root = ProtectedRoot::open(session_gate::Product::CustomerDesk).unwrap();
        assert!(matches!(
            store.snapshot().unwrap().state(),
            LastAcceptedSequence::Known(1)
        ));
        let ready = session_gate::admit().unwrap();
        assert!(matches!(
            session_gate::begin_installation(),
            Err(session_gate::Error::Busy)
        ));
        eprintln!("SYSTEM_PROBE_PHASE:STAGE");
        let mut scratch = root.create(Child::Payload).unwrap();
        scratch.write_all(b"unsealed fixture").unwrap();
        root.sync_file(&scratch).unwrap();
        drop(scratch);
        let stage = root.stage_lease().unwrap();
        assert!(store.discard_ready(&ready).is_err());
        assert!(root.read(Child::Payload).is_ok());
        drop(stage);
        store.discard_ready(&ready).unwrap();
        assert!(matches!(
            root.read(Child::Payload),
            Err(session_gate::Error::MissingGate)
        ));
        drop(ready);
        eprintln!("SYSTEM_PROBE_PHASE:PENDING");
        let transaction = session_gate::begin_installation().unwrap();
        assert!(apply_pending());
        assert!(matches!(
            session_gate::admit(),
            Err(session_gate::Error::Busy)
        ));
        drop(transaction);
        assert!(matches!(
            session_gate::admit(),
            Err(session_gate::Error::Pending)
        ));
        assert!(session_gate::initialize().is_err());
        assert!(apply_pending());
        // Dispose only this freshly created synthetic root, after all handles
        // close. No production cleanup/reset API gains gate/state authority.
        eprintln!("SYSTEM_PROBE_PHASE:CLEANUP");
        let path = root.path(Child::StageLock).parent().unwrap().to_owned();
        drop(root);
        drop(store);
        for name in [
            "admission-v1.lock",
            "state-v1.journal",
            "staging-v1.lock",
            "accepted-sequence-v1",
        ] {
            std::fs::remove_file(path.join(name)).unwrap();
        }
        std::fs::remove_dir(path).unwrap();
        println!("NATIVE_SYSTEM_DESK_BOOTSTRAP_STAGE_PENDING_PASS");
    }
}

fn native_record(manifest: &super::manifest::Manifest, code: Option<u32>) -> [u8; 160] {
    let mut record = [0; 160];
    record[..8].copy_from_slice(b"OGNAT1\0\0");
    record[8..16].copy_from_slice(&manifest.release_sequence.to_be_bytes());
    record[16..56].copy_from_slice(manifest.source_sha.as_bytes());
    record[56..120].copy_from_slice(manifest.sha256.as_bytes());
    record[120] = if code.is_some() { 2 } else { 1 };
    record[124..128].copy_from_slice(&code.unwrap_or(u32::MAX).to_be_bytes());
    let digest = Sha256::digest(&record[..128]);
    record[128..].copy_from_slice(&digest);
    record
}
fn read_native_record(mut file: File) -> Result<[u8; 160], Error> {
    if file.metadata().map_err(|_| Error::Installer)?.len() != 160 {
        return Err(Error::Installer);
    }
    let mut record = [0; 160];
    file.read_exact(&mut record).map_err(|_| Error::Installer)?;
    if record[..8] != *b"OGNAT1\0\0"
        || record[128..] != Sha256::digest(&record[..128])[..]
        || !matches!(record[120], 1 | 2)
        || record[121..124] != [0; 3]
    {
        return Err(Error::Installer);
    }
    Ok(record)
}
fn native_result(
    root: &ProtectedRoot,
    manifest: &super::manifest::Manifest,
) -> Result<Option<u32>, Error> {
    let record = match root.read(Child::NativeState) {
        Ok(file) => read_native_record(file)?,
        Err(session_gate::Error::MissingGate) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if record[120] != 2 {
        return Err(Error::Installer);
    }
    let code = u32::from_be_bytes(record[124..128].try_into().map_err(|_| Error::Installer)?);
    if code == u32::MAX || record != native_record(manifest, Some(code)) {
        return Err(Error::Installer);
    }
    Ok(Some(code))
}
