use super::*;
use hbb_common::sodiumoxide::{self, crypto::sign};
use hbb_common::tokio::io::AsyncWriteExt;
use serde_json::json;
use std::{fs, path::{Path, PathBuf}, process::Command, sync::{mpsc, Mutex}, time::Duration};

pub(super) struct DropObserver {
    writer: usize, lease: std::sync::Weak<StageLease>, context: std::sync::Weak<StoreContext>,
    path: PathBuf, seen: Arc<std::sync::atomic::AtomicUsize>,
}
impl DropObserver {
    fn watch(writer: &File, lease: &Arc<StageLease>, context: &Arc<StoreContext>, path: &Path,
        seen: &Arc<std::sync::atomic::AtomicUsize>) -> Self {
        #[cfg(target_os = "macos")]
        let number = { use std::os::unix::io::AsRawFd; writer.as_raw_fd() as usize };
        #[cfg(target_os = "windows")]
        let number = { use std::os::windows::io::AsRawHandle; writer.as_raw_handle() as usize };
        Self { writer: number, lease: Arc::downgrade(lease), context: Arc::downgrade(context),
            path: path.to_owned(), seen: Arc::clone(seen) }
    }
}
impl Drop for DropObserver {
    fn drop(&mut self) {
        // Nothing may open a new FD/handle before this exact closed-handle probe.
        #[cfg(target_os = "macos")]
        {
            let result = unsafe { hbb_common::libc::fcntl(self.writer as i32, hbb_common::libc::F_GETFD) };
            let error = std::io::Error::last_os_error().raw_os_error();
            assert_eq!(result, -1, "writer still open before guard release");
            assert_eq!(error, Some(hbb_common::libc::EBADF));
        }
        #[cfg(target_os = "windows")]
        {
            use windows::Win32::Foundation::{GetHandleInformation, GetLastError, HANDLE, ERROR_INVALID_HANDLE};
            let mut flags = 0;
            let result = unsafe { GetHandleInformation(HANDLE(self.writer as *mut std::ffi::c_void), &mut flags) };
            let error = unsafe { GetLastError() };
            assert!(result.is_err(), "writer still open before guard release");
            assert_eq!(error, ERROR_INVALID_HANDLE);
        }
        assert_eq!(self.context.strong_count(), 1, "job must still own the only root guard");
        assert_eq!(self.lease.strong_count(), 1, "job must still own the only lease guard");
        child(&self.path, "busy");
        self.seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}

pub(super) struct CreationPause { entered: mpsc::Sender<()>, resume: Mutex<mpsc::Receiver<()>> }
impl CreationPause {
    pub(super) fn wait(&self) -> Result<(), ()> {
        self.entered.send(()).map_err(|_| ())?;
        self.resume.lock().unwrap().recv_timeout(Duration::from_secs(10)).map_err(|_| ())
    }
}

fn run<T>(future: impl Future<Output = T>) -> T {
    tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap().block_on(future)
}
fn state_parent() -> PathBuf {
    PathBuf::from(std::env::var_os("ONGROW_STORE_TEST_ROOT").expect("isolated store test root"))
}
fn tls_root() -> PathBuf {
    PathBuf::from(std::env::var_os("ONGROW_STORE_TLS_ROOT").expect("isolated store TLS root"))
}
fn origin() -> String { std::env::var("ONGROW_STORE_TLS_ORIGIN").expect("isolated store TLS origin") }
fn ca() -> Vec<u8> { fs::read(tls_root().join("cert.pem")).unwrap() }
fn requests() -> Vec<String> { serde_json::from_slice(&fs::read(tls_root().join("requests.json")).unwrap()).unwrap() }
fn reset() { fs::write(tls_root().join("requests.json"), b"[]").unwrap(); }
fn rejects<T>(value: Result<T, Error>, expected: Option<Error>) {
    match value {
        Ok(_) => panic!("protected operation unexpectedly accepted"),
        Err(error) => if let Some(expected) = expected { assert_eq!(error, expected); },
    }
}

struct Fixture { path: PathBuf, public: sign::PublicKey, secret: sign::SecretKey }
impl Fixture {
    fn at(name: &str) -> Self {
        sodiumoxide::init().unwrap();
        let path = state_parent().join(name);
        fs::create_dir(&path).unwrap();
        #[cfg(target_os = "macos")]
        { use std::os::unix::fs::PermissionsExt; fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap(); }
        let (public, secret) = sign::gen_keypair();
        Self { path, public, secret }
    }
    fn identity(&self) -> InstallationIdentity {
        InstallationIdentity::trusted(Product::SupportConsole, Platform::WindowsX64, Channel::Lab,
            8, &self.public.0, &origin(), "/releases/").unwrap()
    }
    fn store(&self, bootstrap: bool) -> ProtectedStore {
        ProtectedStore::fixture(&self.path, self.identity(), bootstrap, &ca(), 100).unwrap()
    }
    fn payload(&self) -> PathBuf { self.path.join("stage-v1.payload") }
    fn publish(&self, chunked: bool, sequence: u64) -> (Vec<u8>, [u8; 64], Vec<u8>) {
        let payload = b"synthetic MSI bytes for console store\0".repeat(5000);
        let manifest = json!({"schema_version":1,"product":"support-console","platform":"windows-x64",
            "channel":"lab","release_sequence":sequence,"upstream_version":"1.2.3","source_sha":"a".repeat(40),
            "issued_at":90,"expires_at":110,"filename":"synthetic.msi","size":payload.len(),
            "sha256":format!("{:x}",Sha256::digest(&payload)),
            "download_url":format!("{}/releases/{sequence}/synthetic.msi",origin())});
        let raw = serde_json::to_vec(&manifest).unwrap();
        let mut domain = super::super::SIGNATURE_DOMAIN.to_vec();
        domain.extend_from_slice(&raw);
        let signature = sign::sign_detached(&domain, &self.secret).to_bytes();
        self.routes(&raw, &signature, &payload, sequence, chunked);
        (raw, signature, payload)
    }
    fn routes(&self, raw: &[u8], signature: &[u8], payload: &[u8], sequence: u64, chunked: bool) {
        let routes = json!({"/releases/manifest.json":{"body":raw},
            "/releases/manifest.sig":{"body":signature},
            format!("/releases/{sequence}/synthetic.msi"): {"body":payload,"chunked":chunked}});
        fs::write(tls_root().join("routes.json"), serde_json::to_vec(&routes).unwrap()).unwrap();
        reset();
    }
}

#[test]
fn state_reopen_and_identity_binding() {
    let fixture = Fixture::at("reopen");
    let store = fixture.store(true);
    assert!(matches!(store.snapshot().unwrap().state(), LastAcceptedSequence::Known(8)));
    let before = fs::read(fixture.path.join("accepted-sequence-v1")).unwrap();
    drop(store);
    assert!(matches!(fixture.store(false).snapshot().unwrap().state(), LastAcceptedSequence::Known(8)));
    rejects(ProtectedStore::fixture(&fixture.path, fixture.identity(), true, &ca(), 100), Some(Error::State));
    assert_eq!(fs::read(fixture.path.join("accepted-sequence-v1")).unwrap(), before);
    let desk = Fixture::at("desk-mismatch");
    let mut identity = desk.identity();
    identity.product = Product::CustomerDesk;
    rejects(ProtectedStore::fixture(&desk.path, identity, true, &ca(), 100), Some(Error::Trust(session_gate::Error::Untrusted)));
    assert_eq!(fs::read_dir(&desk.path).unwrap().count(), 0);
}

#[test]
fn invalid_state_and_partial_bootstrap_block_before_network() {
    for case in ["missing-sequence", "corrupt", "extra", "version", "product", "platform", "channel",
                 "reserved", "missing-lock", "empty-lock", "invalid-lock", "lock-extra"] {
        let fixture = Fixture::at(case);
        let store = fixture.store(true);
        let state = fixture.path.join("accepted-sequence-v1");
        let lock = fixture.path.join("staging-v1.lock");
        let mut bytes = fs::read(&state).unwrap();
        match case {
            "missing-sequence" => fs::remove_file(&state).unwrap(),
            "missing-lock" => fs::remove_file(&lock).unwrap(),
            "empty-lock" => fs::write(&lock, []).unwrap(),
            "invalid-lock" => fs::write(&lock, [2]).unwrap(),
            "lock-extra" => fs::write(&lock, [1, 0]).unwrap(),
            "extra" => { bytes.push(0); fs::write(&state, &bytes).unwrap(); },
            _ => {
                let index = match case { "version" => 5, "product" => 8, "platform" => 9, "channel" => 10, "reserved" => 11, _ => 55 };
                bytes[index] ^= 1;
                if case != "corrupt" { let checksum = Sha256::digest(&bytes[..24]); bytes[24..56].copy_from_slice(&checksum); }
                fs::write(&state, &bytes).unwrap();
            }
        }
        reset();
        rejects(store.snapshot(), None);
        rejects(run(store.download(true)), None);
        assert!(requests().is_empty());
        assert!(!fixture.payload().exists());
        let state_before = fs::read(&state).ok();
        let lock_before = fs::read(&lock).ok();
        rejects(ProtectedStore::fixture(&fixture.path, fixture.identity(), true, &ca(), 100), Some(Error::State));
        assert_eq!(fs::read(&state).ok(), state_before);
        assert_eq!(fs::read(&lock).ok(), lock_before);
    }
    let fixture = Fixture::at("partial-existing-state");
    fs::write(fixture.path.join("accepted-sequence-v1"), b"partial").unwrap();
    rejects(ProtectedStore::fixture(&fixture.path, fixture.identity(), true, &ca(), 100), Some(Error::State));
    assert!(!fixture.path.join("staging-v1.lock").exists());
}

#[test]
fn cross_process_stage_lock_child() {
    let Some(path) = std::env::var_os("ONGROW_STORE_CHILD_ROOT") else { state_parent(); return; };
    let root = ProtectedRoot::fixture(Path::new(&path), session_gate::Product::SupportConsole).unwrap();
    // Reading version byte zero while the stage lease is held is part of the
    // contract on Windows as well as macOS. Locks use offset one on Windows.
    let mut version = [0];
    root.read(Child::StageLock).unwrap().read_exact(&mut version).unwrap();
    assert_eq!(version, [1]);
    let outcome = root.stage_lease();
    match std::env::var("ONGROW_STORE_CHILD_EXPECT").unwrap().as_str() {
        "busy" => assert!(matches!(outcome, Err(session_gate::Error::Busy))),
        "free" => { let lease = outcome.unwrap(); root.read(Child::StageLock).unwrap().read_exact(&mut version).unwrap(); drop(lease); },
        _ => panic!("invalid child expectation"),
    }
}
fn child(path: &Path, expected: &str) {
    let result = Command::new(std::env::current_exe().unwrap()).args(["--exact",
        "ongrow_update::protected_store::tests::cross_process_stage_lock_child", "--nocapture"])
        .env("ONGROW_STORE_CHILD_ROOT", path).env("ONGROW_STORE_CHILD_EXPECT", expected).output().unwrap();
    assert!(result.status.success(), "native child failed: {}", String::from_utf8(result.stdout).unwrap());
}

fn write_input(mut pending: PendingStage) -> StageJobInput {
    assert!(pending.work.is_none());
    StageJobInput { writer: pending.writer.take(), observer: None, identity: pending.identity.take(),
        operation: Operation::Write(b"must not be written".to_vec()),
        lease: Arc::clone(&pending.lease), context: Arc::clone(&pending.context) }
}
fn seal_input(mut pending: PendingStage) -> WorkResult {
    assert!(pending.work.is_none());
    WorkResult { writer: pending.writer.take().unwrap(), observer: None, identity: pending.identity.take().unwrap(),
        count: 0, _lease: Arc::clone(&pending.lease), _context: Arc::clone(&pending.context) }
}
fn tls_pending(store: &ProtectedStore) -> (PendingStage, VerifiedTransfer) {
    run(async {
        let mut pending = store.begin_stage().unwrap();
        let transfer = match store.runtime.check(&mut pending, 100, store.snapshot().unwrap().state(), true).await.unwrap() {
            CheckOutcome::Downloaded(transfer) => transfer, _ => panic!("expected original TLS transfer"),
        };
        (pending, transfer)
    })
}
fn after_job_drop(fixture: &Fixture, seen: &Arc<std::sync::atomic::AtomicUsize>, payload: &[u8]) {
    assert_eq!(seen.load(std::sync::atomic::Ordering::SeqCst), 1);
    child(&fixture.path, "free");
    assert_eq!(fs::read(fixture.payload()).unwrap(), payload);
    assert!(matches!(fixture.store(false).snapshot().unwrap().state(), LastAcceptedSequence::Known(8)));
}

#[test]
fn actual_write_closure_drop_closes_writer_before_native_guards() {
    let fixture = Fixture::at("drop-write-job");
    let store = fixture.store(true);
    let pending = run(async {
        let mut pending = store.begin_stage().unwrap();
        pending.write_all(b"existing payload").await.unwrap();
        pending
    }); // The entire Tokio runtime has stopped before any raw-FD observation.
    let mut input = write_input(pending);
    assert!(input.writer.is_some());
    let seen = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    input.observer = Some(DropObserver::watch(input.writer.as_ref().unwrap(), &input.lease, &input.context, &fixture.path, &seen));
    let job = write_job(input); // This is the production factory, not a copied closure.
    drop(store); // No client, runtime, or extra strong root/lease reference survives.
    drop(job);
    after_job_drop(&fixture, &seen, b"existing payload");
}

#[test]
fn actual_seal_closure_drop_closes_writer_before_native_guards() {
    let fixture = Fixture::at("drop-seal-job");
    let store = fixture.store(true);
    let (_, _, payload) = fixture.publish(false, 9);
    let (pending, transfer) = tls_pending(&store);
    let mut input = seal_input(pending);
    let seen = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    input.observer = Some(DropObserver::watch(&input.writer, &input._lease, &input._context, &fixture.path, &seen));
    let job = seal_job(input, transfer);
    drop(store);
    drop(job);
    after_job_drop(&fixture, &seen, &payload);
}

#[test]
fn actual_early_seal_error_closes_writer_before_native_guards() {
    let fixture = Fixture::at("early-seal-error");
    let store = fixture.store(true);
    let (_, _, payload) = fixture.publish(false, 9);
    let (pending, transfer) = tls_pending(&store);
    let mut input = seal_input(pending);
    let sequence = input._context.root.read(Child::Sequence).unwrap();
    input.identity = input._context.root.identity(&sequence).unwrap();
    drop(sequence); // All OS-handle opening is finished before recording the writer.
    let seen = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    input.observer = Some(DropObserver::watch(&input.writer, &input._lease, &input._context, &fixture.path, &seen));
    let job = seal_job(input, transfer);
    drop(store);
    rejects(job(), Some(Error::Trust(session_gate::Error::Untrusted)));
    after_job_drop(&fixture, &seen, &payload);
}

#[test]
fn stage_lock_is_independent_of_sessions_and_pending() {
    let fixture = Fixture::at("parallel-sessions");
    let store = fixture.store(true);
    session_gate::store_handles::fixture_gate_initialize(&fixture.path).unwrap();
    let session = session_gate::store_handles::fixture_session(&fixture.path).unwrap();
    let lease = store.context.root.stage_lease().unwrap();
    assert!(matches!(store.snapshot().unwrap().state(), LastAcceptedSequence::Known(8)));
    let other = session_gate::store_handles::fixture_session(&fixture.path).unwrap();
    assert_eq!(fs::read(fixture.path.join("state-v1.journal")).unwrap(), [0]);
    child(&fixture.path, "busy");
    drop(lease);
    child(&fixture.path, "free");
    drop(other);
    drop(session);
}

#[test]
fn real_tls_file_seal_and_same_owner_boundary() {
    for (label, chunked) in [("download-fixed", false), ("download-chunked", true)] {
        let fixture = Fixture::at(label);
        let store = fixture.store(true);
        let (raw, signature, payload) = fixture.publish(chunked, 9);
        let mut ticket = match run(store.download(true)).unwrap() { StageOutcome::Sealed(ticket) => ticket, _ => panic!("expected sealed download") };
        assert_eq!(ticket.raw_manifest, raw);
        assert_eq!(ticket.signature, signature);
        assert_eq!(ticket.candidate.manifest().release_sequence, 9);
        ticket.reverify().unwrap();
        assert_eq!(requests(), ["/releases/manifest.json", "/releases/manifest.sig", "/releases/9/synthetic.msi"]);
        assert!(matches!(store.snapshot().unwrap().state(), LastAcceptedSequence::Known(8)));
        child(&fixture.path, "busy");
        #[cfg(target_os = "windows")]
        {
            assert!(fs::OpenOptions::new().write(true).open(fixture.payload()).is_err());
            assert!(fs::rename(fixture.payload(), fixture.path.join("replacement")).is_err());
            assert!(fs::remove_file(fixture.payload()).is_err());
            ticket.reverify().unwrap();
        }
        #[cfg(target_os = "macos")]
        {
            let mut modified = payload.clone(); modified[0] ^= 1;
            fs::write(fixture.payload(), &modified).unwrap();
            rejects(ticket.reverify(), Some(Error::Verification(super::super::Error::PayloadHash)));
        }
        drop(ticket);
        child(&fixture.path, "free");
        assert!(fixture.payload().exists());
        reset();
        rejects(run(store.download(true)), None);
        assert!(requests().is_empty());
    }
    println!("NATIVE_STORE_TLS_HANDLE_PASS");
}

#[test]
fn not_due_no_update_and_metadata_error_do_not_reserve_slot() {
    let fixture = Fixture::at("lazy-slot");
    let store = fixture.store(true);
    fixture.publish(false, 8);
    rejects(run(store.download(false)), Some(Error::Transfer(RuntimeError::NotDue)));
    assert!(requests().is_empty()); assert!(!fixture.payload().exists());
    assert!(matches!(run(store.download(true)).unwrap(), StageOutcome::NoUpdate));
    assert_eq!(requests(), ["/releases/manifest.json", "/releases/manifest.sig"]);
    assert!(!fixture.payload().exists());
    let (raw, mut signature, payload) = fixture.publish(false, 9);
    signature[0] ^= 1;
    fixture.routes(&raw, &signature, &payload, 9, false);
    rejects(run(store.download(true)), Some(Error::Transfer(RuntimeError::Verification(super::super::Error::InvalidSignature))));
    assert_eq!(requests(), ["/releases/manifest.json", "/releases/manifest.sig"]);
    assert!(!fixture.payload().exists());
    fixture.publish(false, 9);
    assert!(matches!(run(store.download(true)).unwrap(), StageOutcome::Sealed(_)));
    assert!(fixture.payload().exists());
}

#[test]
fn seal_fresh_state_time_and_tampered_bytes_rejected() {
    for case in ["seal-state", "seal-time", "seal-short", "seal-long", "seal-hash", "seal-replacement"] {
        let fixture = Fixture::at(case);
        let store = fixture.store(true);
        fixture.publish(false, 9);
        run(async {
            let mut pending = store.begin_stage().unwrap();
            let transfer = match store.runtime.check(&mut pending, 100, store.snapshot().unwrap().state(), true).await.unwrap() {
                CheckOutcome::Downloaded(transfer) => transfer, _ => panic!("expected transfer"),
            };
            match case {
                "seal-state" => { fs::write(fixture.path.join("accepted-sequence-v1"), store.context.identity.record(9)).unwrap(); },
                "seal-time" => store.context.test_time.as_ref().unwrap().store(111, std::sync::atomic::Ordering::SeqCst),
                _ => {
                    #[cfg(target_os = "windows")]
                    assert!(fs::OpenOptions::new().write(true).open(fixture.payload()).is_err());
                    #[cfg(target_os = "macos")]
                    match case {
                        "seal-replacement" => {
                            fs::rename(fixture.payload(), fixture.path.join("old-payload")).unwrap();
                            fs::write(fixture.payload(), b"replacement").unwrap();
                        }
                        _ => {
                            let mut bytes = fs::read(fixture.payload()).unwrap();
                            match case { "seal-short" => { bytes.pop(); }, "seal-long" => bytes.push(0), _ => bytes[0] ^= 1 }
                            fs::write(fixture.payload(), bytes).unwrap();
                        }
                    }
                }
            }
            #[cfg(target_os = "windows")]
            if case.starts_with("seal-s") && case != "seal-state" || case == "seal-long" || case == "seal-hash" || case == "seal-replacement" {
                pending.seal(transfer).await.unwrap(); return;
            }
            rejects(pending.seal(transfer).await, None);
        });
        assert!(fixture.payload().exists());
    }
}

#[test]
fn cancelled_creation_holds_kernel_lease_until_io_finishes() {
    let fixture = Fixture::at("cancel-creation");
    let mut store = fixture.store(true);
    fixture.publish(false, 9);
    let (entered, receiver) = mpsc::channel();
    let (resume, receive) = mpsc::channel();
    Arc::get_mut(&mut store.context).unwrap().creation_pause = Some(Arc::new(CreationPause { entered, resume: Mutex::new(receive) }));
    run(async {
        let task = tokio::spawn(async move { store.download(true).await });
        tokio::task::spawn_blocking(move || receiver.recv_timeout(Duration::from_secs(5)).unwrap()).await.unwrap();
        task.abort();
        assert!(matches!(task.await, Err(error) if error.is_cancelled()));
        child(&fixture.path, "busy");
        assert!(!fixture.payload().exists());
        resume.send(()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            let root = ProtectedRoot::fixture(&fixture.path, session_gate::Product::SupportConsole).unwrap();
            if root.stage_lease().is_ok() { break; }
            assert!(std::time::Instant::now() < deadline, "creator did not release lease");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        child(&fixture.path, "free");
        assert!(fixture.payload().exists());
        reset();
        rejects(fixture.store(false).download(true).await, None);
        assert!(requests().is_empty());
    });
}

#[test]
fn sink_error_and_untrusted_partial_slot_are_never_deleted() {
    let fixture = Fixture::at("sink-failure");
    let store = fixture.store(true);
    fixture.publish(false, 9);
    let sentinel = fixture.path.join("foreign-sentinel");
    fs::write(&sentinel, b"must remain").unwrap();
    run(async {
        let mut pending = store.begin_stage().unwrap();
        pending.write_all(b"partial").await.unwrap();
        let writer = pending.writer.take().unwrap();
        drop(writer);
        pending.writer = Some(fs::File::open(fixture.payload()).unwrap());
        rejects(store.runtime.check(&mut pending, 100, store.snapshot().unwrap().state(), true).await.map_err(Error::from),
            Some(Error::Transfer(RuntimeError::Sink)));
        drop(pending);
    });
    assert_eq!(fs::read(fixture.payload()).unwrap(), b"partial");
    assert_eq!(fs::read(sentinel).unwrap(), b"must remain");
    reset(); rejects(run(store.download(true)), None); assert!(requests().is_empty());
}

#[test]
fn child_symlink_hardlink_and_foreign_write_fail_closed() {
    let child_alias = if cfg!(target_os = "windows") { "child-reparse" } else { "child-symlink" };
    for case in [child_alias, "child-hardlink", "child-write", "root-symlink", "root-write"] {
        let fixture = Fixture::at(case);
        let store = fixture.store(true);
        let state = fixture.path.join("accepted-sequence-v1");
        match case {
            "child-hardlink" => fs::hard_link(&state, fixture.path.join("linked-state")).unwrap(),
            "child-symlink" | "child-reparse" => {
                fs::rename(&state, fixture.path.join("original-state")).unwrap();
                #[cfg(target_os = "macos")]
                std::os::unix::fs::symlink(fixture.path.join("original-state"), &state).unwrap();
                #[cfg(target_os = "windows")]
                { assert!(Command::new("cmd").args(["/C", "mklink", "/J"]).arg(&state).arg(&fixture.path).output().unwrap().status.success()); }
            }
            #[cfg(target_os = "macos")]
            "child-write" | "root-write" => {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(if case == "root-write" { &fixture.path } else { &state }, fs::Permissions::from_mode(0o777)).unwrap();
            }
            #[cfg(target_os = "windows")]
            "child-write" | "root-write" => {
                let path = if case == "root-write" { &fixture.path } else { &state };
                assert!(Command::new("icacls").arg(path).args(["/grant", "*S-1-1-0:(W)"]).output().unwrap().status.success());
            }
            "root-symlink" => {
                let linked = state_parent().join("alias-root");
                #[cfg(target_os = "macos")]
                std::os::unix::fs::symlink(&fixture.path, &linked).unwrap();
                #[cfg(target_os = "windows")]
                assert!(Command::new("cmd").args(["/C", "mklink", "/J"]).arg(&linked).arg(&fixture.path).output().unwrap().status.success());
                rejects(ProtectedStore::fixture(&linked, fixture.identity(), false, &ca(), 100), None);
                continue;
            }
            _ => unreachable!(),
        }
        reset(); rejects(run(store.download(true)), None); assert!(requests().is_empty());
        assert!(!fixture.payload().exists());
    }
}
