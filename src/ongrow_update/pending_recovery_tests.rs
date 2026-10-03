use super::*;
use std::{fs, io::{BufRead, Write}, path::{Path, PathBuf}, process::{Child, Command, Stdio},
    sync::{atomic::{AtomicU32, Ordering}, mpsc}, thread, time::Duration};

static NEXT: AtomicU32 = AtomicU32::new(0);
const GATE: &str = "admission-v1.lock";
const JOURNAL: &str = "state-v1.journal";

fn error<T>(result: Result<T, Error>, expected: Error) {
    assert!(matches!(result, Err(value) if value == expected));
}
fn recover(path: &Path) -> Result<PendingRecoveryLease, Error> {
    pending_recovery_lease(os::fixture_acquire(path, true)?)
}
fn identity(path: &Path) -> (u64, u64) {
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = fs::metadata(path).unwrap();
        (metadata.dev(), metadata.ino())
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::{Foundation::HANDLE, Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle}};
        let file = fs::File::open(path).unwrap();
        let mut information = BY_HANDLE_FILE_INFORMATION::default();
        unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut information) }.unwrap();
        (information.dwVolumeSerialNumber as u64,
            (information.nFileIndexHigh as u64) << 32 | information.nFileIndexLow as u64)
    }
}
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let base = PathBuf::from(std::env::var_os("ONGROW_PENDING_RECOVERY_TEST_ROOT")
            .expect("isolated pending recovery test root"));
        let path = base.join(format!("{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::SeqCst)));
        fs::create_dir(&path).unwrap();
        let fixture = Self(path);
        #[cfg(target_os = "macos")]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&fixture.0, fs::Permissions::from_mode(0o700)).unwrap();
        }
        os::fixture_initialize(&fixture.0).unwrap();
        fixture
    }
    fn pending(&self) {
        let mut owner = os::fixture_acquire(&self.0, true).unwrap();
        owner.mark_pending().unwrap();
    }
    fn snapshot(&self) -> Vec<(Vec<u8>, (u64, u64))> {
        [GATE, JOURNAL].iter().map(|name| {
            let path = self.0.join(name);
            (fs::read(&path).unwrap(), identity(&path))
        }).collect()
    }
    fn child(&self, action: &str) -> ProbeChild {
        let name = format!("{}::native_child", module_path!().split_once("::").unwrap().1);
        let child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &name, "--nocapture"])
            .env("ONGROW_PENDING_RECOVERY_CHILD_ROOT", &self.0)
            .env("ONGROW_PENDING_RECOVERY_CHILD_ACTION", action)
            .stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
        let mut guard = ProbeChild { child, reader: None };
        let output = guard.child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        guard.reader = Some(thread::spawn(move || {
            for line in std::io::BufReader::new(output).lines() {
                let line = line.unwrap();
                // Serial libtest may prefix the marker with its test label.
                if line.ends_with("PENDING_RECOVERY_CHILD_READY") { let _ = tx.send(()); }
            }
        }));
        rx.recv_timeout(Duration::from_secs(10)).expect("bounded child handshake");
        guard
    }
}
impl Drop for Fixture {
    fn drop(&mut self) { fs::remove_dir_all(&self.0).unwrap(); }
}
struct ProbeChild { child: Child, reader: Option<thread::JoinHandle<()>> }
impl ProbeChild {
    fn crash(&mut self) {
        self.child.kill().unwrap();
        assert!(!self.child.wait().unwrap().success());
    }
    fn finish(&mut self) {
        self.child.stdin.take().unwrap().write_all(b"exit\n").unwrap();
        // The child has no work beyond reading this line. Bound even a broken
        // helper; Drop still kills and waits on assertion failure.
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() { assert!(status.success()); break; }
            assert!(std::time::Instant::now() < deadline, "bounded child exit");
            thread::yield_now();
        }
    }
}
impl Drop for ProbeChild {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() { let _ = reader.join(); }
    }
}

#[test]
fn unconfigured_production_api_is_disabled_without_fixture_effects() {
    let fixture = Fixture::new();
    let before = fixture.snapshot();
    error(acquire_pending_recovery(), Error::Disabled);
    assert_eq!(before, fixture.snapshot());
}
#[test]
fn ready_rejects_without_byte_or_identity_changes() {
    let fixture = Fixture::new();
    let before = fixture.snapshot();
    error(recover(&fixture.0), Error::Untrusted);
    assert_eq!(before, fixture.snapshot());
}
#[test]
fn real_shared_owner_blocks_recovery_and_ready_still_rejects() {
    let fixture = Fixture::new();
    let before = fixture.snapshot();
    let mut child = fixture.child("shared");
    error(recover(&fixture.0), Error::Busy);
    child.finish();
    error(recover(&fixture.0), Error::Untrusted);
    assert_eq!(before, fixture.snapshot());
}
#[test]
fn existing_pending_keeps_identity_and_blocks_real_other_processes() {
    let fixture = Fixture::new();
    fixture.pending();
    let before = fixture.snapshot();
    let lease = recover(&fixture.0).unwrap();
    for action in ["busy-shared", "busy-recovery"] {
        let mut child = fixture.child(action);
        child.finish();
    }
    assert_eq!(before, fixture.snapshot());
    drop(lease);
    assert_eq!(before, fixture.snapshot());
}
#[test]
fn drop_preserves_pending_and_next_owner_can_recover() {
    let fixture = Fixture::new();
    fixture.pending();
    let before = fixture.snapshot();
    drop(recover(&fixture.0).unwrap());
    error(os::fixture_acquire(&fixture.0, false), Error::Pending);
    let _next = recover(&fixture.0).unwrap();
    assert_eq!(before, fixture.snapshot());
}
#[test]
fn real_pending_owner_crash_releases_only_kernel_lock() {
    let fixture = Fixture::new();
    let mut owner = fixture.child("pending");
    let before = fixture.snapshot();
    error(recover(&fixture.0), Error::Busy);
    owner.crash();
    error(os::fixture_acquire(&fixture.0, false), Error::Pending);
    let _lease = recover(&fixture.0).unwrap();
    assert_eq!(before, fixture.snapshot());
}
#[test]
fn z_real_recovery_owner_crash_allows_another_pending_owner() {
    let fixture = Fixture::new();
    fixture.pending();
    let before = fixture.snapshot();
    let mut owner = fixture.child("recovery");
    error(recover(&fixture.0), Error::Busy);
    owner.crash();
    error(os::fixture_acquire(&fixture.0, false), Error::Pending);
    let _lease = recover(&fixture.0).unwrap();
    assert_eq!(before, fixture.snapshot());
    println!("NATIVE_PENDING_RECOVERY_PASS");
}
#[test]
fn missing_and_malformed_journal_reject_without_repair() {
    let fixture = Fixture::new();
    for malformed in [vec![], vec![2], vec![1, 0]] {
        fs::write(fixture.0.join(JOURNAL), &malformed).unwrap();
        let before = fixture.snapshot();
        error(recover(&fixture.0), Error::Untrusted);
        assert_eq!(before, fixture.snapshot());
    }
    let gate_before = (fs::read(fixture.0.join(GATE)).unwrap(), identity(&fixture.0.join(GATE)));
    fs::remove_file(fixture.0.join(JOURNAL)).unwrap();
    error(recover(&fixture.0), Error::MissingGate);
    assert!(!fixture.0.join(JOURNAL).exists());
    assert_eq!(gate_before, (fs::read(fixture.0.join(GATE)).unwrap(), identity(&fixture.0.join(GATE))));
}
#[test]
fn shared_lock_with_pending_cannot_enter_private_factory() {
    let fixture = Fixture::new();
    let shared = os::fixture_acquire(&fixture.0, false).unwrap();
    // Only our fixture is modified. This specifically tests the exclusive
    // guard rather than relying on the Ready rejection.
    fs::write(fixture.0.join(JOURNAL), [1]).unwrap();
    let before = fixture.snapshot();
    error(pending_recovery_lease(shared), Error::Untrusted);
    assert_eq!(before, fixture.snapshot());
    let _exclusive = recover(&fixture.0).unwrap();
}
#[test]
fn native_child() {
    let Some(path) = std::env::var_os("ONGROW_PENDING_RECOVERY_CHILD_ROOT") else { return; };
    let path = PathBuf::from(path);
    let action = std::env::var("ONGROW_PENDING_RECOVERY_CHILD_ACTION").unwrap();
    let mut lock = None;
    let mut recovery = None;
    match action.as_str() {
        "shared" => lock = Some(os::fixture_acquire(&path, false).unwrap()),
        "pending" => {
            let mut owner = os::fixture_acquire(&path, true).unwrap();
            owner.mark_pending().unwrap();
            lock = Some(owner);
        }
        "recovery" => recovery = Some(recover(&path).unwrap()),
        "busy-shared" => error(os::fixture_acquire(&path, false), Error::Busy),
        "busy-recovery" => error(recover(&path), Error::Busy),
        _ => panic!("unknown own probe action"),
    }
    println!("PENDING_RECOVERY_CHILD_READY");
    std::io::stdout().flush().unwrap();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).unwrap();
    drop(recovery);
    drop(lock);
}
