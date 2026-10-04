//! Test-only guardian, outside the bundle/service replaced by native installers.
use super::*;
use std::{fs, io::{BufRead, Read, Write}, path::{Path, PathBuf},
    process::{Child, Command, Stdio}, sync::{atomic::{AtomicU32, Ordering}, mpsc},
    thread, time::{Duration, Instant}};

static NEXT: AtomicU32 = AtomicU32::new(0);

// Disposable fixture construction only. This never changes the process token,
// token privileges/groups, existing state, or any production gate decision.
#[cfg(target_os = "windows")]
mod windows_fixture {
    use super::*;
    use windows::Win32::{
        Foundation::{CloseHandle, HANDLE},
        Security::{DuplicateTokenEx, EqualSid, GetLengthSid, GetTokenInformation,
            IsValidSid, SetTokenInformation, SecurityImpersonation, TokenImpersonation,
            TokenOwner, TokenUser, TOKEN_ADJUST_DEFAULT, TOKEN_DUPLICATE,
            TOKEN_IMPERSONATE, TOKEN_OWNER, TOKEN_QUERY, TOKEN_USER},
        System::Threading::{GetCurrentProcess, GetCurrentThread, OpenProcessToken,
            OpenThreadToken, SetThreadToken},
    };

    struct Token(HANDLE);
    impl Drop for Token {
        fn drop(&mut self) { unsafe { let _ = CloseHandle(self.0); } }
    }
    fn process_token() -> Result<Token, Error> {
        let mut token = Token(HANDLE::default());
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY | TOKEN_DUPLICATE, &mut token.0) }
            .map_err(|_| Error::Io)?;
        Ok(token)
    }
    fn thread_token() -> Result<Option<Token>, Error> {
        let mut token = Token(HANDLE::default());
        match unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY | TOKEN_IMPERSONATE, true, &mut token.0) } {
            Ok(()) => Ok(Some(token)),
            // HRESULT_FROM_WIN32(ERROR_NO_TOKEN), not arbitrary access failure.
            Err(error) if error.code().0 as u32 == 0x8007_03f0 => Ok(None),
            Err(_) => Err(Error::Io),
        }
    }
    fn information(token: &Token, class: windows::Win32::Security::TOKEN_INFORMATION_CLASS) -> Result<Vec<usize>, Error> {
        let mut size = 0;
        let _ = unsafe { GetTokenInformation(token.0, class, None, 0, &mut size) };
        if size == 0 || size > 64 * 1024 { return Err(Error::Untrusted); }
        let mut buffer = vec![0usize; (size as usize + std::mem::size_of::<usize>() - 1) / std::mem::size_of::<usize>()];
        unsafe { GetTokenInformation(token.0, class, Some(buffer.as_mut_ptr().cast()), size, &mut size) }
            .map_err(|_| Error::Io)?;
        Ok(buffer)
    }
    fn owner_snapshot(token: &Token) -> Result<Vec<u8>, Error> {
        let buffer = information(token, TokenOwner)?;
        let sid = unsafe { (*buffer.as_ptr().cast::<TOKEN_OWNER>()).Owner };
        if !unsafe { IsValidSid(sid) }.as_bool() { return Err(Error::Untrusted); }
        let length = unsafe { GetLengthSid(sid) } as usize;
        if length == 0 || length > 256 { return Err(Error::Untrusted); }
        Ok(unsafe { std::slice::from_raw_parts(sid.0.cast::<u8>(), length) }.to_vec())
    }
    struct OwnerScope {
        previous: Option<Token>, _duplicate: Token,
        _thread_bound: std::marker::PhantomData<std::rc::Rc<()>>,
    }
    impl OwnerScope {
        fn current_user() -> Result<Self, Error> {
            let primary = process_token()?;
            let user = information(&primary, TokenUser)?;
            let sid = unsafe { (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid };
            Self::with_owner(&primary, sid)
        }
        fn with_owner(primary: &Token, sid: windows::Win32::Security::PSID) -> Result<Self, Error> {
            if !unsafe { IsValidSid(sid) }.as_bool() { return Err(Error::Untrusted); }
            let previous = thread_token()?;
            let mut duplicate = Token(HANDLE::default());
            unsafe { DuplicateTokenEx(primary.0, TOKEN_QUERY | TOKEN_IMPERSONATE | TOKEN_ADJUST_DEFAULT,
                None, SecurityImpersonation, TokenImpersonation, &mut duplicate.0) }.map_err(|_| Error::Io)?;
            let owner = TOKEN_OWNER { Owner: sid };
            unsafe { SetTokenInformation(duplicate.0, TokenOwner,
                (&owner as *const TOKEN_OWNER).cast(), std::mem::size_of::<TOKEN_OWNER>() as u32) }
                .map_err(|_| Error::Io)?;
            let copied = information(&duplicate, TokenOwner)?;
            let copied_sid = unsafe { (*copied.as_ptr().cast::<TOKEN_OWNER>()).Owner };
            unsafe { EqualSid(sid, copied_sid) }.map_err(|_| Error::Untrusted)?;
            unsafe { SetThreadToken(None, Some(duplicate.0)) }.map_err(|_| Error::Io)?;
            Ok(Self { previous, _duplicate: duplicate, _thread_bound: std::marker::PhantomData })
        }
    }
    impl Drop for OwnerScope {
        fn drop(&mut self) {
            // Test-only fail closed. Never continue fixture operations in an
            // unexpected impersonation context if restoration fails.
            assert!(unsafe { SetThreadToken(None, self.previous.as_ref().map(|token| token.0)) }.is_ok(),
                "fixture thread context restoration failed");
        }
    }
    pub(super) fn bootstrap(path: &Path) -> Result<(), Error> {
        let base = PathBuf::from(std::env::var_os("ONGROW_NATIVE_APPLY_TEST_ROOT").ok_or(Error::Untrusted)?);
        let relative = path.strip_prefix(&base).map_err(|_| Error::Untrusted)?;
        let parts: Vec<_> = relative.components().collect();
        if parts.len() != 2 || path.file_name() != Some(std::ffi::OsStr::new("gate"))
            || !matches!(parts[0], std::path::Component::Normal(name) if name.to_string_lossy().starts_with("guardian-"))
        { return Err(Error::Untrusted); }
        use std::os::windows::fs::MetadataExt;
        for parent in [base.as_path(), path.parent().ok_or(Error::Untrusted)?] {
            let metadata = fs::symlink_metadata(parent).map_err(|_| Error::Untrusted)?;
            if !metadata.is_dir() || metadata.file_attributes() & 0x400 != 0 { return Err(Error::Untrusted); }
        }
        let _scope = OwnerScope::current_user()?;
        // create_dir is exclusive. Existing/partial roots are never repaired.
        fs::create_dir(path).map_err(|_| Error::Untrusted)?;
        os::fixture_initialize(path)
    }

    #[test]
    fn fixture_owner_is_user_and_primary_context_is_unchanged() {
        let primary = process_token().unwrap();
        let before = owner_snapshot(&primary).unwrap();
        assert!(thread_token().unwrap().is_none(), "fresh test thread expected");
        let base = PathBuf::from(std::env::var_os("ONGROW_NATIVE_APPLY_TEST_ROOT").unwrap());
        let parent = base.join(format!("guardian-owner-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::SeqCst)));
        fs::create_dir(&parent).unwrap();
        let path = parent.join("gate");
        bootstrap(&path).unwrap();
        // The ORIGINAL gate checks exact user ownership of root and both files.
        drop(os::fixture_acquire(&path, false).unwrap());
        error(bootstrap(&path), Error::Untrusted);
        assert!(before == owner_snapshot(&primary).unwrap(), "primary owner changed");
        assert!(thread_token().unwrap().is_none(), "fixture context leaked");
        fs::remove_dir_all(parent).unwrap();
        println!("WINDOWS_FIXTURE_USER_OWNER_PRIMARY_UNCHANGED_PASS");
    }
    #[test]
    fn original_gate_still_rejects_wrong_root_and_journal_owner() {
        use windows::Win32::Security::{CreateWellKnownSid, PSID, WinBuiltinAdministratorsSid};
        let primary = process_token().unwrap();
        let before = owner_snapshot(&primary).unwrap();
        let mut storage = [0usize; 16];
        let admins = PSID(storage.as_mut_ptr().cast());
        let mut length = std::mem::size_of_val(&storage) as u32;
        unsafe { CreateWellKnownSid(WinBuiltinAdministratorsSid, None, Some(admins), &mut length) }.unwrap();
        let base = PathBuf::from(std::env::var_os("ONGROW_NATIVE_APPLY_TEST_ROOT").unwrap());
        let parent = base.join(format!("guardian-negative-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::SeqCst)));
        fs::create_dir(&parent).unwrap();
        let wrong_root = parent.join("wrong-root");
        {
            let _scope = OwnerScope::with_owner(&primary, admins).unwrap();
            fs::create_dir(&wrong_root).unwrap();
            error(os::fixture_initialize(&wrong_root), Error::Untrusted);
        }
        assert!(!wrong_root.join("state-v1.journal").exists(), "rejected root was mutated");
        let wrong_file = parent.join("wrong-file");
        {
            let _scope = OwnerScope::current_user().unwrap();
            fs::create_dir(&wrong_file).unwrap();
        }
        {
            let _scope = OwnerScope::with_owner(&primary, admins).unwrap();
            error(os::fixture_initialize(&wrong_file), Error::Untrusted);
        }
        assert!(wrong_file.join("state-v1.journal").exists(), "journal rejection was not exercised");
        assert!(!wrong_file.join("admission-v1.lock").exists(), "partial bootstrap continued");
        assert!(before == owner_snapshot(&primary).unwrap(), "primary owner changed");
        assert!(thread_token().unwrap().is_none(), "fixture context leaked");
        fs::remove_dir_all(parent).unwrap();
        println!("WINDOWS_WRONG_ROOT_AND_FILE_OWNER_STILL_REJECTED_PASS");
    }
}

fn bootstrap(path: &Path) -> Result<(), Error> {
    #[cfg(target_os = "windows")]
    { windows_fixture::bootstrap(path) }
    #[cfg(target_os = "macos")]
    { os::fixture_initialize(path) }
}
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
        "bootstrap" => bootstrap(&path).map(|_| "Ready"),
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
