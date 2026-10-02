use super::*;
use std::{fs, io::{BufRead, Write}, path::{Path, PathBuf}, process::{Child, Command, Stdio}, sync::atomic::{AtomicU32, Ordering}};

static NEXT: AtomicU32 = AtomicU32::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let base = PathBuf::from(std::env::var_os("ONGROW_GATE_TEST_ROOT").expect("isolated test root"));
        let path = base.join(format!("{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::SeqCst)));
        fs::create_dir(&path).unwrap();
        #[cfg(target_os = "macos")]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        }
        os::fixture_initialize(&path).unwrap();
        Self(path)
    }
    fn child(&self, action: &str) -> Child {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "session_gate::tests::native_child", "--nocapture"])
            .env("ONGROW_GATE_CHILD_ROOT", &self.0).env("ONGROW_GATE_CHILD_ACTION", action)
            .stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
        let output = child.stdout.take().unwrap();
        let mut reader = std::io::BufReader::new(output);
        let mut line = String::new();
        loop {
            assert!(reader.read_line(&mut line).unwrap() > 0, "child did not acquire gate");
            if line.contains("GATE_READY") { break; }
            line.clear();
        }
        // Keep the read end alive until wait/kill. Dropping it here makes the
        // Rust test runner's final result output fail with BrokenPipe.
        child.stdout = Some(reader.into_inner());
        child
    }
}
impl Drop for Fixture { fn drop(&mut self) { fs::remove_dir_all(&self.0).unwrap(); } }

fn error<T>(result: Result<T, Error>, expected: Error) {
    assert!(matches!(result, Err(value) if value == expected));
}

#[test]
fn default_policy_has_no_side_effects() {
    assert_eq!(startup_policy().unwrap(), None);
    let lease = admit().unwrap();
    let _clone = lease.clone();
    error(begin_installation(), Error::Disabled);
    error(initialize(), Error::Disabled);
}

#[test]
fn real_shared_sessions_block_exclusive_until_last_drop() {
    let fixture = Fixture::new();
    let one = os::fixture_acquire(&fixture.0, false).unwrap();
    let two = os::fixture_acquire(&fixture.0, false).unwrap();
    error(os::fixture_acquire(&fixture.0, true), Error::Busy);
    drop(one);
    error(os::fixture_acquire(&fixture.0, true), Error::Busy);
    drop(two);
    let _installation = os::fixture_acquire(&fixture.0, true).unwrap();
    error(os::fixture_acquire(&fixture.0, false), Error::Busy);
}

#[test]
fn forward_subtask_keeps_last_shared_lease_after_parent_returns() {
    let fixture = Fixture::new();
    let parent = SessionLease { _lock: Some(Arc::new(os::fixture_acquire(&fixture.0, false).unwrap())) };
    let forward = parent.clone();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (end_tx, end_rx) = std::sync::mpsc::channel();
    let task = std::thread::spawn(move || {
        let _forward = forward;
        ready_tx.send(()).unwrap();
        end_rx.recv().unwrap();
    });
    ready_rx.recv().unwrap();
    drop(parent);
    error(os::fixture_acquire(&fixture.0, true), Error::Busy);
    end_tx.send(()).unwrap();
    task.join().unwrap();
    assert!(os::fixture_acquire(&fixture.0, true).is_ok());
}

#[test]
fn cross_process_shared_lock_and_crash_pending() {
    let fixture = Fixture::new();
    let mut child = fixture.child("session");
    let second = os::fixture_acquire(&fixture.0, false).unwrap();
    error(os::fixture_acquire(&fixture.0, true), Error::Busy);
    child.kill().unwrap();
    child.wait().unwrap();
    error(os::fixture_acquire(&fixture.0, true), Error::Busy);
    drop(second);
    let mut installer = fixture.child("pending");
    error(os::fixture_acquire(&fixture.0, false), Error::Busy);
    installer.kill().unwrap();
    installer.wait().unwrap();
    error(os::fixture_acquire(&fixture.0, false), Error::Pending);
    // This is a test-only verified health capability. No production mint exists.
    Transaction { lock: os::fixture_acquire(&fixture.0, true).unwrap() }.commit_healthy(VerifiedHealth(())).unwrap();
    assert!(os::fixture_acquire(&fixture.0, false).is_ok());
    let mut next = fixture.child("session");
    error(os::fixture_acquire(&fixture.0, true), Error::Busy);
    next.stdin.take().unwrap().write_all(b"exit\n").unwrap();
    assert!(next.wait().unwrap().success());
    assert!(os::fixture_acquire(&fixture.0, true).is_ok());
}

#[test]
fn malformed_missing_and_reinitialized_state_stay_blocked() {
    let fixture = Fixture::new();
    error(os::fixture_initialize(&fixture.0), Error::Untrusted);
    for malformed in [vec![], vec![2], vec![0, 0]] {
        fs::write(fixture.0.join("state-v1.journal"), malformed).unwrap();
        error(os::fixture_acquire(&fixture.0, false), Error::Untrusted);
        error(os::fixture_acquire(&fixture.0, true), Error::Untrusted);
    }
    fs::remove_file(fixture.0.join("state-v1.journal")).unwrap();
    error(os::fixture_acquire(&fixture.0, false), Error::MissingGate);
    error(os::fixture_initialize(&fixture.0), Error::Untrusted);
    assert!(!fixture.0.join("state-v1.journal").exists());
    error(os::fixture_acquire(&fixture.0, false), Error::MissingGate);
}

#[test]
fn partial_bootstrap_never_repairs_or_unlocks_state() {
    let fixture = Fixture::new();
    fs::remove_file(fixture.0.join("admission-v1.lock")).unwrap();
    error(os::fixture_initialize(&fixture.0), Error::Untrusted);
    assert!(!fixture.0.join("admission-v1.lock").exists());
    error(os::fixture_acquire(&fixture.0, false), Error::MissingGate);
}

#[test]
fn hardlinked_gate_and_journal_are_rejected() {
    let fixture = Fixture::new();
    for name in ["admission-v1.lock", "state-v1.journal"] {
        let alias = fixture.0.join("alias");
        fs::hard_link(fixture.0.join(name), &alias).unwrap();
        error(os::fixture_acquire(&fixture.0, false), Error::Untrusted);
        fs::remove_file(alias).unwrap();
    }
}

#[cfg(target_os = "macos")]
#[test]
fn symlinks_and_foreign_writable_permissions_are_rejected() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let fixture = Fixture::new();
    fs::set_permissions(&fixture.0, fs::Permissions::from_mode(0o777)).unwrap();
    error(os::fixture_acquire(&fixture.0, false), Error::Untrusted);
    fs::set_permissions(&fixture.0, fs::Permissions::from_mode(0o700)).unwrap();
    let original = fixture.0.join("admission-v1.lock");
    let moved = fixture.0.join("moved");
    fs::rename(&original, &moved).unwrap();
    symlink(&moved, &original).unwrap();
    error(os::fixture_acquire(&fixture.0, false), Error::Untrusted);
}

#[cfg(target_os = "macos")]
#[test]
fn darwin_acl_empty_read_allow_and_write_allow_are_checked() {
    let fixture = Fixture::new();
    let gate = fixture.0.join("admission-v1.lock");
    // Empty ACL already passed fixture bootstrap. Only this throw-away file
    // receives ACL changes; no parent, real app or system setting is touched.
    assert!(Command::new("/bin/chmod").args(["+a", "everyone allow read"]).arg(&gate).status().unwrap().success());
    assert!(os::fixture_acquire(&fixture.0, false).is_ok());
    assert!(Command::new("/bin/chmod").args(["+a", "everyone allow write"]).arg(&gate).status().unwrap().success());
    error(os::fixture_acquire(&fixture.0, false), Error::Untrusted);
    error(os::fixture_acquire(&fixture.0, true), Error::Untrusted);
}

#[cfg(target_os = "macos")]
#[test]
fn darwin_journal_acl_and_unreadable_state_are_rejected() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    let journal = fixture.0.join("state-v1.journal");
    fs::set_permissions(&journal, fs::Permissions::from_mode(0o000)).unwrap();
    error(os::fixture_acquire(&fixture.0, false), Error::Untrusted);
    fs::set_permissions(&journal, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(Command::new("/bin/chmod").args(["+a", "everyone allow write"]).arg(&journal).status().unwrap().success());
    error(os::fixture_acquire(&fixture.0, false), Error::Untrusted);
    error(os::fixture_acquire(&fixture.0, true), Error::Untrusted);
}

#[cfg(target_os = "macos")]
#[test]
fn darwin_root_foreign_write_acl_is_rejected() {
    let fixture = Fixture::new();
    assert!(Command::new("/bin/chmod").args(["+a", "everyone allow write"]).arg(&fixture.0).status().unwrap().success());
    error(os::fixture_acquire(&fixture.0, false), Error::Untrusted);
    error(os::fixture_acquire(&fixture.0, true), Error::Untrusted);
}

#[test]
fn failed_installation_drop_does_not_erase_pending() {
    let fixture = Fixture::new();
    let mut lock = os::fixture_acquire(&fixture.0, true).unwrap();
    lock.mark_pending().unwrap();
    drop(lock);
    error(os::fixture_acquire(&fixture.0, false), Error::Pending);
    let mut retry = os::fixture_acquire(&fixture.0, true).unwrap();
    error(retry.mark_pending(), Error::Pending);
}

#[test]
fn journal_changes_keep_fixed_gate_and_journal_identity() {
    let fixture = Fixture::new();
    fn identity(path: &Path) -> u64 {
        #[cfg(target_os = "macos")]
        { use std::os::unix::fs::MetadataExt; fs::metadata(path).unwrap().ino() }
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::io::AsRawHandle;
            use windows::Win32::{Foundation::HANDLE, Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle}};
            let file = fs::File::open(path).unwrap();
            let mut info = BY_HANDLE_FILE_INFORMATION::default();
            unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info) }.unwrap();
            (info.nFileIndexHigh as u64) << 32 | info.nFileIndexLow as u64
        }
    }
    let before = identity(&fixture.0.join("admission-v1.lock"));
    let journal = identity(&fixture.0.join("state-v1.journal"));
    let mut lock = os::fixture_acquire(&fixture.0, true).unwrap();
    lock.mark_pending().unwrap();
    drop(lock);
    error(os::fixture_acquire(&fixture.0, false), Error::Pending);
    Transaction { lock: os::fixture_acquire(&fixture.0, true).unwrap() }.commit_healthy(VerifiedHealth(())).unwrap();
    assert_eq!(before, identity(&fixture.0.join("admission-v1.lock")));
    assert_eq!(journal, identity(&fixture.0.join("state-v1.journal")));
}

#[test]
fn native_child() {
    let Some(path) = std::env::var_os("ONGROW_GATE_CHILD_ROOT") else { return; };
    let action = std::env::var("ONGROW_GATE_CHILD_ACTION").unwrap();
    let mut lease = os::fixture_acquire(&PathBuf::from(path), action == "pending").unwrap();
    if action == "pending" { lease.mark_pending().unwrap(); }
    println!("GATE_READY");
    std::io::stdout().flush().unwrap();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).unwrap();
    drop(lease);
}
