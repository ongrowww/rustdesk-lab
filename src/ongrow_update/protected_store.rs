//! Fixed-slot protected store and signed attempt. No acceptance advance, cleanup or apply.
#[path = "protected_store/attempt.rs"]
mod attempt;
use super::{
    manifest::{Channel, Platform, Product},
    runtime::{CheckOutcome, RuntimeError, RuntimePolicy, UpdateRuntime, VerifiedTransfer},
    session_gate::{self, store_handles::{Child, FileIdentity, ProtectedRoot, StageLease}},
    verify_manifest, Context, DownloadLocation, LastAcceptedSequence, VerifiedCandidate,
};
use hbb_common::tokio::{self, io::AsyncWrite};
use sha2::{Digest, Sha256};
use std::{fs::File, future::Future, io::{Read, Seek, SeekFrom, Write}, pin::Pin, sync::Arc,
    task::{Context as TaskContext, Poll}, time::{SystemTime, UNIX_EPOCH}};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Error {
    Trust(session_gate::Error), State, Clock, Io, Verification(super::Error), Transfer(RuntimeError),
}
impl From<session_gate::Error> for Error { fn from(value: session_gate::Error) -> Self { Self::Trust(value) } }
impl From<super::Error> for Error { fn from(value: super::Error) -> Self { Self::Verification(value) } }
impl From<RuntimeError> for Error { fn from(value: RuntimeError) -> Self { Self::Transfer(value) } }

/// Supplied only by the future trusted bootstrap. No configuration/environment reader.
pub(crate) struct InstallationIdentity {
    product: Product, platform: Platform, channel: Channel, baked_sequence: u64,
    key: [u8; 32], location: DownloadLocation, origin: String, prefix: String,
}
impl InstallationIdentity {
    pub(crate) fn trusted(product: Product, platform: Platform, channel: Channel,
        baked_sequence: u64, key: &[u8], origin: &str, prefix: &str) -> Result<Self, Error> {
        let key = key.try_into().map_err(|_| super::Error::InvalidKey)?;
        let location = DownloadLocation::new(origin, prefix)?;
        Ok(Self { product, platform, channel, baked_sequence, key, location,
            origin: origin.to_owned(), prefix: prefix.to_owned() })
    }
    fn gate_product(&self) -> session_gate::Product {
        match self.product {
            Product::CustomerDesk => session_gate::Product::CustomerDesk,
            Product::SupportConsole => session_gate::Product::SupportConsole,
        }
    }
    fn runtime_policy(&self) -> Result<RuntimePolicy, Error> {
        Ok(RuntimePolicy::new(self.product, self.platform, self.channel, &self.key,
            self.baked_sequence, &self.origin, &self.prefix)?)
    }
    fn context(&self, now: u64, snapshot: SequenceSnapshot) -> Context<'_> {
        Context { product: self.product, platform: self.platform, channel: self.channel,
            baked_current_sequence: self.baked_sequence, now_unix_seconds: now,
            last_accepted_sequence: snapshot.state, download_location: &self.location }
    }
    fn record(&self, sequence: u64) -> [u8; 56] {
        let mut bytes = [0u8; 56];
        bytes[..8].copy_from_slice(b"OGSEQ1\0\0");
        bytes[8] = match self.product { Product::CustomerDesk => 1, Product::SupportConsole => 2 };
        bytes[9] = match self.platform { Platform::WindowsX64 => 1 };
        bytes[10] = match self.channel { Channel::Lab => 1, Channel::Stable => 2 };
        bytes[16..24].copy_from_slice(&sequence.to_be_bytes());
        let checksum = Sha256::digest(&bytes[..24]);
        bytes[24..].copy_from_slice(&checksum);
        bytes
    }
}

pub(crate) struct SequenceSnapshot { state: LastAcceptedSequence }
impl SequenceSnapshot {
    pub(crate) fn state(&self) -> LastAcceptedSequence { self.state }
}

struct StoreContext {
    root: ProtectedRoot, identity: InstallationIdentity,
    #[cfg(all(test, ongrow_update_store_probe))]
    test_time: Option<std::sync::atomic::AtomicU64>,
    #[cfg(all(test, ongrow_update_store_probe))]
    creation_pause: Option<Arc<tests::CreationPause>>,
}
impl StoreContext {
    fn now(&self) -> Result<u64, Error> {
        #[cfg(all(test, ongrow_update_store_probe))]
        if let Some(now) = &self.test_time { return Ok(now.load(std::sync::atomic::Ordering::SeqCst)); }
        Ok(SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| Error::Clock)?.as_secs())
    }
    fn snapshot(&self) -> Result<SequenceSnapshot, Error> {
        let mut lock = self.root.read(Child::StageLock)?;
        if lock.metadata().map_err(|_| Error::Io)?.len() != 1 { return Err(Error::State); }
        let mut version = [0];
        lock.read_exact(&mut version).map_err(|_| Error::State)?;
        if version != [1] { return Err(Error::State); }
        let mut file = self.root.read(Child::Sequence)?;
        if file.metadata().map_err(|_| Error::Io)?.len() != 56 { return Err(Error::State); }
        let mut bytes = [0; 56];
        file.read_exact(&mut bytes).map_err(|_| Error::State)?;
        let sequence = u64::from_be_bytes(bytes[16..24].try_into().map_err(|_| Error::State)?);
        if bytes != self.identity.record(sequence) { return Err(Error::State); }
        Ok(SequenceSnapshot { state: LastAcceptedSequence::Known(sequence) })
    }
}

pub(crate) struct ProtectedStore { context: Arc<StoreContext>, runtime: UpdateRuntime }
pub(crate) enum StageOutcome { Sealed(SealedStageTicket), NoUpdate }
impl ProtectedStore {
    fn assemble(root: ProtectedRoot, identity: InstallationIdentity) -> Result<Self, Error> {
        let runtime = UpdateRuntime::new(identity.runtime_policy()?)?;
        Ok(Self { context: Arc::new(StoreContext { root, identity,
            #[cfg(all(test, ongrow_update_store_probe))]
            test_time: None,
            #[cfg(all(test, ongrow_update_store_probe))]
            creation_pause: None,
        }), runtime })
    }
    pub(crate) fn open(identity: InstallationIdentity) -> Result<Self, Error> {
        let root = ProtectedRoot::open(identity.gate_product())?;
        Self::assemble(root, identity)
    }
    /// Explicit, first creation only. Never creates the root, repairs or advances.
    pub(crate) fn bootstrap(identity: InstallationIdentity) -> Result<Self, Error> {
        let root = ProtectedRoot::open(identity.gate_product())?;
        Self::initialize(&root, &identity)?;
        Self::assemble(root, identity)
    }
    fn initialize(root: &ProtectedRoot, identity: &InstallationIdentity) -> Result<(), Error> {
        for child in [Child::Sequence, Child::StageLock] {
            match root.read(child) {
                Err(session_gate::Error::MissingGate) => {},
                _ => return Err(Error::State),
            }
        }
        let mut state = root.create(Child::Sequence)?;
        state.write_all(&identity.record(identity.baked_sequence)).map_err(|_| Error::Io)?;
        root.sync_file(&state)?;
        let mut lock = root.create(Child::StageLock)?;
        lock.write_all(&[1]).map_err(|_| Error::Io)?;
        root.sync_file(&lock)?;
        root.sync_directory()?;
        Ok(())
    }
    pub(crate) fn snapshot(&self) -> Result<SequenceSnapshot, Error> { self.context.snapshot() }
    fn begin_stage(&self) -> Result<PendingStage, Error> {
        self.snapshot()?;
        let lease = Arc::new(self.context.root.stage_lease()?);
        // Re-read after acquiring: partial or modified bootstrap remains unusable.
        self.snapshot()?;
        for child in [Child::Payload, Child::Attempt] {
            match self.context.root.read(child) {
                Err(session_gate::Error::MissingGate) => {},
                Err(error) => return Err(error.into()),
                Ok(_) => return Err(Error::State),
            }
        }
        Ok(PendingStage { writer: None,
            #[cfg(all(test, ongrow_update_store_probe))]
            observer: None,
            identity: None, work: None,
            context: Arc::clone(&self.context), lease })
    }
    pub(crate) async fn download(&self, manual: bool) -> Result<StageOutcome, Error> {
        let mut pending = self.begin_stage()?;
        let snapshot = self.snapshot()?;
        match self.runtime.check(&mut pending, self.context.now()?, snapshot.state, manual).await? {
            CheckOutcome::Downloaded(transfer) => Ok(StageOutcome::Sealed(pending.seal(transfer).await?)),
            CheckOutcome::NoUpdate => Ok(StageOutcome::NoUpdate),
        }
    }
    #[cfg(all(test, ongrow_update_store_probe))]
    fn fixture(path: &std::path::Path, identity: InstallationIdentity, bootstrap: bool,
        certificate: &[u8], now: u64) -> Result<Self, Error> {
        let root = ProtectedRoot::fixture(path, identity.gate_product())?;
        if bootstrap { Self::initialize(&root, &identity)?; }
        let runtime = UpdateRuntime::store_probe(identity.runtime_policy()?, certificate)?;
        Ok(Self { runtime, context: Arc::new(StoreContext { root, identity,
            test_time: Some(std::sync::atomic::AtomicU64::new(now)), creation_pause: None }) })
    }
}

/// The writer is private and consumed by seal. Drop never deletes the fixed slot.
struct PendingStage {
    writer: Option<File>,
    #[cfg(all(test, ongrow_update_store_probe))]
    observer: Option<tests::DropObserver>,
    identity: Option<FileIdentity>,
    work: Option<tokio::task::JoinHandle<std::io::Result<WorkResult>>>,
    lease: Arc<StageLease>, context: Arc<StoreContext>,
}
// Field order closes the writer before releasing its kernel lease and root,
// including when a cancelled JoinHandle discards an already-completed output.
struct WorkResult {
    writer: File,
    #[cfg(all(test, ongrow_update_store_probe))]
    observer: Option<tests::DropObserver>,
    identity: FileIdentity, count: usize,
    _lease: Arc<StageLease>, _context: Arc<StoreContext>,
}
enum Operation { Write(Vec<u8>), Flush }
// Whole-container capture preserves this order even before the job starts.
struct StageJobInput {
    writer: Option<File>,
    #[cfg(all(test, ongrow_update_store_probe))]
    observer: Option<tests::DropObserver>,
    identity: Option<FileIdentity>, operation: Operation,
    lease: Arc<StageLease>, context: Arc<StoreContext>,
}
fn sink_error() -> std::io::Error { std::io::Error::new(std::io::ErrorKind::Other, "protected stage I/O rejected") }
fn write_job(input: StageJobInput) -> impl FnOnce() -> std::io::Result<WorkResult> {
    move || {
        let mut input = input;
        // Locals taken afterward close before the still-owned input guards on error.
        let (mut writer, identity) = match (input.writer.take(), input.identity.take()) {
            (Some(writer), Some(identity)) => (writer, identity),
            (None, None) if matches!(input.operation, Operation::Write(_)) => {
                #[cfg(all(test, ongrow_update_store_probe))]
                if let Some(pause) = &input.context.creation_pause { pause.wait().map_err(|_| sink_error())?; }
                let file = input.context.root.create(Child::Payload).map_err(|_| sink_error())?;
                let identity = input.context.root.identity(&file).map_err(|_| sink_error())?;
                (file, identity)
            }
            _ => return Err(sink_error()),
        };
        let count = match input.operation {
            Operation::Write(bytes) => writer.write(&bytes)?,
            Operation::Flush => { writer.flush()?; 0 },
        };
        Ok(WorkResult { writer,
            #[cfg(all(test, ongrow_update_store_probe))]
            observer: input.observer.take(),
            identity, count, _lease: input.lease, _context: input.context })
    }
}
fn seal_job(input: WorkResult, transfer: VerifiedTransfer) -> impl FnOnce() -> Result<SealedStageTicket, Error> {
    move || {
        let mut input = input;
        input.writer.flush().map_err(|_| Error::Io)?;
        input._context.root.same(&input.writer, &input.identity)?;
        input._context.root.sync_file(&input.writer)?;
        drop(input.writer);
        let reader = input._context.root.read(Child::Payload)?;
        input._context.root.same(&reader, &input.identity)?;
        let (raw, signature) = transfer.signed_manifest();
        let snapshot = input._context.snapshot()?;
        let candidate = verify_manifest(raw, signature, &input._context.identity.key,
            &input._context.identity.context(input._context.now()?, snapshot))?;
        hash_handle(&reader, &candidate)?;
        input._context.root.same(&reader, &input.identity)?;
        let attempt = attempt::persist(&input._context, raw, signature)?;
        Ok(SealedStageTicket { reader, attempt, identity: input.identity, context: input._context, _lease: input._lease,
            candidate, raw_manifest: raw.to_vec(), signature: *signature })
    }
}
impl PendingStage {
    fn start(&mut self, operation: Operation) {
        let input = StageJobInput { writer: self.writer.take(),
            #[cfg(all(test, ongrow_update_store_probe))]
            observer: self.observer.take(),
            identity: self.identity.take(), operation,
            lease: Arc::clone(&self.lease), context: Arc::clone(&self.context) };
        self.work = Some(tokio::task::spawn_blocking(write_job(input)));
    }
    fn finish(&mut self, cx: &mut TaskContext<'_>) -> Poll<std::io::Result<usize>> {
        let Some(work) = self.work.as_mut() else { return Poll::Ready(Err(sink_error())); };
        let result = match Pin::new(work).poll(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(result) => result,
        };
        self.work = None;
        match result {
            Ok(Ok(result)) => {
                self.writer = Some(result.writer);
                #[cfg(all(test, ongrow_update_store_probe))]
                { self.observer = result.observer; }
                self.identity = Some(result.identity);
                Poll::Ready(Ok(result.count))
            }
            _ => Poll::Ready(Err(sink_error())),
        }
    }
}
impl AsyncWrite for PendingStage {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>, bytes: &[u8]) -> Poll<std::io::Result<usize>> {
        if self.work.is_none() {
            if bytes.is_empty() { return Poll::Ready(Ok(0)); }
            self.start(Operation::Write(bytes[..bytes.len().min(64 * 1024)].to_vec()));
        }
        self.finish(cx)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
        if self.work.is_none() {
            if self.writer.is_none() { return Poll::Ready(Ok(())); }
            self.start(Operation::Flush);
        }
        self.finish(cx).map(|result| result.map(|_| ()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
        self.poll_flush(cx)
    }
}
impl PendingStage {
    async fn seal(mut self, transfer: VerifiedTransfer) -> Result<SealedStageTicket, Error> {
        if let Some(work) = self.work.take() {
            let finished = work.await.map_err(|_| Error::Io)?.map_err(|_| Error::Io)?;
            self.writer = Some(finished.writer);
            #[cfg(all(test, ongrow_update_store_probe))]
            { self.observer = finished.observer; }
            self.identity = Some(finished.identity);
        }
        let writer = self.writer.take().ok_or(Error::State)?;
        let identity = self.identity.take().ok_or(Error::State)?;
        let input = WorkResult { writer,
            #[cfg(all(test, ongrow_update_store_probe))]
            observer: self.observer.take(),
            identity, count: 0, _context: self.context, _lease: self.lease };
        tokio::task::spawn_blocking(seal_job(input, transfer)).await.map_err(|_| Error::Io)?
    }
}

fn hash_handle(mut file: &File, candidate: &VerifiedCandidate) -> Result<(), Error> {
    file.seek(SeekFrom::Start(0)).map_err(|_| Error::Io)?;
    let mut verifier = candidate.payload_verifier();
    let mut chunk = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut chunk).map_err(|_| Error::Io)?;
        if count == 0 { break; }
        verifier.update(&chunk[..count])?;
    }
    verifier.finalize()?;
    Ok(())
}

/// A held read-only file plus lease, never installer authority or immutable bytes.
pub(crate) struct SealedStageTicket {
    reader: File, attempt: attempt::StoredAttempt, identity: FileIdentity,
    _lease: Arc<StageLease>, context: Arc<StoreContext>,
    candidate: VerifiedCandidate, raw_manifest: Vec<u8>, signature: [u8; 64],
}
impl SealedStageTicket {
    pub(crate) fn reverify(&mut self) -> Result<(), Error> {
        self.attempt.matches(&self.context, &self.raw_manifest, &self.signature)?;
        self.context.root.same(&self.reader, &self.identity)?;
        let snapshot = self.context.snapshot()?;
        let candidate = verify_manifest(&self.raw_manifest, &self.signature, &self.context.identity.key,
            &self.context.identity.context(self.context.now()?, snapshot))?;
        hash_handle(&self.reader, &candidate)?;
        self.context.root.same(&self.reader, &self.identity)?;
        self.attempt.matches(&self.context, &self.raw_manifest, &self.signature)?;
        Ok(())
    }
}

#[cfg(all(test, ongrow_update_store_probe))]
#[path = "protected_store_tests.rs"]
mod tests;
