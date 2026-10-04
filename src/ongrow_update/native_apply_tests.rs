//! Test-only guardian, outside the bundle/service replaced by native installers.
use super::*;
use std::{fs, io::{BufRead, Read, Write}, path::{Path, PathBuf},
    process::{Child, Command, Stdio}, sync::{atomic::{AtomicU32, Ordering}, mpsc},
    thread, time::{Duration, Instant}};

static NEXT: AtomicU32 = AtomicU32::new(0);
fn begin(path: &Path) -> Result<Transaction, Error> {
    let mut lock = os::fixture_acquire(path, true)?;
    lock.mark_pending()?;
    Ok(Transaction { lock })
}
fn recover(path: &Path) -> Result<PendingRecoveryLease, Error> {
    pending_recovery_lease(os::fixture_acquire(path, true)?)
}
fn error<T>(result: Result<T, Error>, expected: Error) {
    assert!(matches!(result, Err(value) if value == expected));
}
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let base = PathBuf::from(std::env::var_os("ONGROW_NATIVE_APPLY_TEST_ROOT")
            .expect("isolated native apply test root"));
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
    fn child(&self, action: &str, expected: &str) -> ProbeChild {
        let name = format!("{}::native_child", module_path!().split_once("::").unwrap().1);
        let child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &name, "--nocapture"])
            .env("ONGROW_NATIVE_APPLY_CHILD_ROOT", &self.0)
            .env("ONGROW_NATIVE_APPLY_CHILD_ACTION", action)
            .stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
        let mut guard = ProbeChild { child, reader: None };
        let output = guard.child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        guard.reader = Some(thread::spawn(move || {
            for line in std::io::BufReader::new(output.take(4096)).lines() {
                let line = line.unwrap();
                if let Some((_, status)) = line.split_once("NATIVE_APPLY_STATUS=") {
                    let _ = tx.send(status.to_owned());
                }
            }
        }));
        assert_eq!(rx.recv_timeout(Duration::from_secs(10)).expect("bounded child handshake"), expected);
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
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() { assert!(status.success()); break; }
            assert!(Instant::now() < deadline, "bounded child exit");
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
fn session_child_blocks_begin_then_transaction_records_durable_pending() {
    let fixture = Fixture::new();
    let mut session = fixture.child("session", "Ready");
    error(begin(&fixture.0), Error::Busy);
    assert_eq!(fs::read(fixture.0.join("state-v1.journal")).unwrap(), [0]);
    session.finish();
    let transaction = begin(&fixture.0).unwrap();
    assert_eq!(fs::read(fixture.0.join("state-v1.journal")).unwrap(), [1]);
    drop(transaction);
    error(os::fixture_acquire(&fixture.0, false), Error::Pending);
}
#[test]
fn transaction_owner_blocks_real_second_admission_and_begin() {
    let fixture = Fixture::new();
    let mut owner = fixture.child("transaction", "Pending");
    let mut admission = fixture.child("admit", "Busy");
    admission.finish();
    let mut contender = fixture.child("transaction", "Busy");
    contender.finish();
    owner.finish();
    error(os::fixture_acquire(&fixture.0, false), Error::Pending);
}
#[test]
fn guardian_crash_preserves_pending_and_recovery_owns_same_lock() {
    let fixture = Fixture::new();
    let mut owner = fixture.child("transaction", "Pending");
    owner.crash();
    error(os::fixture_acquire(&fixture.0, false), Error::Pending);
    let _lease = recover(&fixture.0).unwrap();
    let mut admission = fixture.child("admit", "Busy");
    admission.finish();
}
#[test]
fn recovery_owner_blocks_other_process_and_drop_never_restores_ready() {
    let fixture = Fixture::new();
    drop(begin(&fixture.0).unwrap());
    let mut owner = fixture.child("recovery", "Pending");
    let mut contender = fixture.child("recovery", "Busy");
    contender.finish();
    owner.finish();
    error(os::fixture_acquire(&fixture.0, false), Error::Pending);
}
#[test]
fn ready_is_not_recovery_authority() {
    let fixture = Fixture::new();
    error(recover(&fixture.0), Error::Untrusted);
    assert_eq!(fs::read(fixture.0.join("state-v1.journal")).unwrap(), [0]);
}
#[test]
fn malformed_journal_rejects_begin_and_recovery_without_repair() {
    let fixture = Fixture::new();
    for bytes in [vec![], vec![2], vec![1, 0]] {
        fs::write(fixture.0.join("state-v1.journal"), &bytes).unwrap();
        error(begin(&fixture.0), Error::Untrusted);
        error(recover(&fixture.0), Error::Untrusted);
        assert_eq!(fs::read(fixture.0.join("state-v1.journal")).unwrap(), bytes);
    }
}
#[test]
fn pending_cannot_be_restarted_as_new_transaction() {
    let fixture = Fixture::new();
    drop(begin(&fixture.0).unwrap());
    error(begin(&fixture.0), Error::Pending);
    drop(recover(&fixture.0).unwrap());
    error(os::fixture_acquire(&fixture.0, false), Error::Pending);
}
#[test]
fn production_api_remains_disabled_and_child_recovery_crash_keeps_pending() {
    let fixture = Fixture::new();
    error(begin_installation(), Error::Disabled);
    error(acquire_pending_recovery(), Error::Disabled);
    drop(begin(&fixture.0).unwrap());
    let mut owner = fixture.child("recovery", "Pending");
    owner.crash();
    error(os::fixture_acquire(&fixture.0, false), Error::Pending);
    let _lease = recover(&fixture.0).unwrap();
    println!("NATIVE_APPLY_GATE_PASS");
}

#[test]
fn native_child() {
    let Some(path) = std::env::var_os("ONGROW_NATIVE_APPLY_CHILD_ROOT") else { return; };
    let path = PathBuf::from(path);
    let action = std::env::var("ONGROW_NATIVE_APPLY_CHILD_ACTION").expect("explicit own child action");
    assert!(action.len() <= 16 && path.as_os_str().len() <= 4096, "bounded probe input");
    let mut session = None;
    let mut transaction = None;
    let mut recovery = None;
    let result = match action.as_str() {
        "bootstrap" => os::fixture_initialize(&path).map(|_| "Ready"),
        "session" => os::fixture_acquire(&path, false).map(|lock| { session = Some(lock); "Ready" }),
        "transaction" => begin(&path).map(|owner| { transaction = Some(owner); "Pending" }),
        "recovery" => recover(&path).map(|lease| { recovery = Some(lease); "Pending" }),
        "admit" => os::fixture_acquire(&path, false).map(|_| "Ready"),
        _ => panic!("unknown own probe action"),
    };
    let status = match result { Ok(status) => status.to_owned(), Err(error) => format!("{error:?}") };
    println!("NATIVE_APPLY_STATUS={status}");
    std::io::stdout().flush().unwrap();
    // Even the pipe read has a deadline and a size limit. An abandoned parent
    // releases only the OS lock, never the durable Pending journal.
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut line = String::new();
        let result = std::io::BufReader::new(std::io::stdin().take(64)).read_line(&mut line);
        let _ = tx.send((result, line));
    });
    let (result, line) = rx.recv_timeout(Duration::from_secs(300)).expect("bounded guardian lifetime");
    assert!(result.is_ok() && line == "exit\n", "explicit bounded guardian exit required");
    drop(recovery);
    drop(transaction);
    drop(session);
}
