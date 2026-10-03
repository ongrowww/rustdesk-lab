//! Inactive until a trusted app caller supplies a policy and an untrusted sink.
//! This scheduler does not grant session, staging or installer authority.
use super::{
    manifest::{Channel, Platform, Product},
    Context, DownloadLocation, Error, LastAcceptedSequence, VerifiedCandidate, MAX_MANIFEST_BYTES,
};
use hbb_common::tokio::{
    self,
    io::{AsyncWrite, AsyncWriteExt},
};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub(crate) const MAX_PAYLOAD_BYTES: u64 = 512 * 1024 * 1024;
const INITIAL: Duration = Duration::from_secs(30);
const SUCCESS: Duration = Duration::from_secs(24 * 60 * 60);
const RETRY: Duration = Duration::from_secs(30 * 60);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const CHECK_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeError {
    Verification(Error),
    Client,
    Transport,
    Status,
    ResponseUrl,
    Encoding,
    ContentLength,
    MetadataSize,
    PayloadLimit,
    Deadline,
    Sink,
    Busy,
    NotDue,
    ClockRange,
}

impl From<Error> for RuntimeError {
    fn from(error: Error) -> Self {
        Self::Verification(error)
    }
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "OnGROW transfer rejected: {:?}", self)
    }
}
impl std::error::Error for RuntimeError {}

/// Only locally trusted future bootstrap code may construct this policy.
/// No default endpoint, environment or ordinary user configuration is read.
pub(crate) struct RuntimePolicy {
    product: Product,
    platform: Platform,
    channel: Channel,
    public_key: [u8; 32],
    current_sequence: u64,
    location: DownloadLocation,
    manifest_url: String,
    signature_url: String,
}

impl RuntimePolicy {
    pub(crate) fn new(
        product: Product,
        platform: Platform,
        channel: Channel,
        public_key: &[u8],
        current_sequence: u64,
        origin: &str,
        prefix: &str,
    ) -> Result<Self, Error> {
        let public_key = public_key.try_into().map_err(|_| Error::InvalidKey)?;
        let location = DownloadLocation::new(origin, prefix)?;
        Ok(Self {
            product,
            platform,
            channel,
            public_key,
            current_sequence,
            manifest_url: format!("{origin}{prefix}manifest.json"),
            signature_url: format!("{origin}{prefix}manifest.sig"),
            location,
        })
    }

    fn context(&self, now: u64, last_sequence: LastAcceptedSequence) -> Context<'_> {
        Context {
            product: self.product,
            platform: self.platform,
            channel: self.channel,
            now_unix_seconds: now,
            baked_current_sequence: self.current_sequence,
            last_accepted_sequence: last_sequence,
            download_location: &self.location,
        }
    }
}

struct ScheduleState {
    next: Instant,
    last_now: Instant,
    active: bool,
    exhausted: bool,
}

pub(crate) struct CheckScheduler {
    state: Arc<Mutex<ScheduleState>>,
    clock: Arc<dyn Fn() -> Instant + Send + Sync>,
}

impl CheckScheduler {
    pub(crate) fn new() -> Result<Self, RuntimeError> {
        Self::with_clock(Arc::new(Instant::now))
    }

    fn with_clock(clock: Arc<dyn Fn() -> Instant + Send + Sync>) -> Result<Self, RuntimeError> {
        let now = clock();
        let next = now.checked_add(INITIAL).ok_or(RuntimeError::ClockRange)?;
        Ok(Self {
            state: Arc::new(Mutex::new(ScheduleState {
                next,
                last_now: now,
                active: false,
                exhausted: false,
            })),
            clock,
        })
    }

    fn acquire(&self, manual: bool) -> Result<CheckLease, RuntimeError> {
        let mut state = self.state.lock().expect("scheduler poisoned");
        let now = (self.clock)().max(state.last_now);
        state.last_now = now;
        if state.exhausted {
            return Err(RuntimeError::ClockRange);
        }
        if state.active {
            return Err(RuntimeError::Busy);
        }
        if !manual && now < state.next {
            return Err(RuntimeError::NotDue);
        }
        // Reserve the next retry before side effects, including arithmetic failure.
        now.checked_add(RETRY).ok_or(RuntimeError::ClockRange)?;
        state.active = true;
        Ok(CheckLease {
            state: Arc::clone(&self.state),
            clock: Arc::clone(&self.clock),
            success: false,
        })
    }
}

struct CheckLease {
    state: Arc<Mutex<ScheduleState>>,
    clock: Arc<dyn Fn() -> Instant + Send + Sync>,
    success: bool,
}

impl Drop for CheckLease {
    fn drop(&mut self) {
        let mut state = self.state.lock().expect("scheduler poisoned");
        let now = (self.clock)().max(state.last_now);
        state.last_now = now;
        // At an unrepresentable future instant stay fail-closed, never busy-loop.
        match now.checked_add(if self.success { SUCCESS } else { RETRY }) {
            Some(next) => state.next = next,
            None => state.exhausted = true,
        }
        state.active = false;
    }
}

/// Certifies this completed transfer only. Does not certify a mutable file/path.
/// A future apply operation must reverify a protected handle independently.
pub(crate) struct VerifiedTransfer {
    candidate: VerifiedCandidate,
    raw_manifest: Vec<u8>,
    detached_signature: [u8; 64],
}
impl VerifiedTransfer {
    pub(crate) fn candidate(&self) -> &VerifiedCandidate {
        &self.candidate
    }

    /// Exact authenticated bytes for a future verifier with fresh trusted state.
    /// These bounded read-only bytes confer no staging or installer authority.
    pub(crate) fn signed_manifest(&self) -> (&[u8], &[u8; 64]) {
        (&self.raw_manifest, &self.detached_signature)
    }
}

pub(crate) enum CheckOutcome {
    Downloaded(VerifiedTransfer),
    /// Existing verifier authenticated exact bytes, context and time before
    /// rejecting a non-increasing sequence. No payload or sink access follows.
    NoUpdate,
}

pub(crate) struct UpdateRuntime {
    policy: RuntimePolicy,
    scheduler: CheckScheduler,
    client: reqwest::Client,
    deadline: Duration,
}

fn client_builder() -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
}

impl UpdateRuntime {
    pub(crate) fn new(policy: RuntimePolicy) -> Result<Self, RuntimeError> {
        let client = client_builder().build().map_err(|_| RuntimeError::Client)?;
        Ok(Self {
            policy,
            scheduler: CheckScheduler::new()?,
            client,
            deadline: CHECK_TIMEOUT,
        })
    }

    /// Never spawns a thread or creates a Tokio runtime. Cancellation drops the lease.
    /// Any partial sink content after any error remains untrusted and caller-owned.
    pub(crate) async fn check<W: AsyncWrite + Unpin>(
        &self,
        sink: &mut W,
        now: u64,
        last_sequence: LastAcceptedSequence,
        manual: bool,
    ) -> Result<CheckOutcome, RuntimeError> {
        if matches!(last_sequence, LastAcceptedSequence::Unknown) {
            return Err(Error::UnknownReleaseState.into());
        }
        let mut lease = self.scheduler.acquire(manual)?;
        let result = tokio::time::timeout(self.deadline, self.transfer(sink, now, last_sequence))
            .await
            .map_err(|_| RuntimeError::Deadline)?;
        if result.is_ok() {
            lease.success = true;
        }
        result
    }

    async fn response(
        &self,
        url: &str,
        cap: u64,
        exact: Option<u64>,
    ) -> Result<(reqwest::Response, Option<u64>), RuntimeError> {
        let response = self
            .client
            .get(url)
            .header(reqwest::header::ACCEPT_ENCODING, "identity")
            .send()
            .await
            .map_err(|_| RuntimeError::Transport)?;
        if response.status() != reqwest::StatusCode::OK {
            return Err(RuntimeError::Status);
        }
        if response.url().as_str() != url {
            return Err(RuntimeError::ResponseUrl);
        }
        for value in response
            .headers()
            .get_all(reqwest::header::CONTENT_ENCODING)
        {
            if value.as_bytes() != b"identity" {
                return Err(RuntimeError::Encoding);
            }
        }
        let mut declared = None;
        let lengths = response.headers().get_all(reqwest::header::CONTENT_LENGTH);
        if lengths.iter().count() > 1 {
            return Err(RuntimeError::ContentLength);
        }
        if let Some(value) = lengths.iter().next() {
            if response
                .headers()
                .contains_key(reqwest::header::TRANSFER_ENCODING)
            {
                return Err(RuntimeError::ContentLength);
            }
            let length = value
                .to_str()
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                .ok_or(RuntimeError::ContentLength)?;
            if length > cap || exact.is_some_and(|expected| length != expected) {
                return Err(RuntimeError::ContentLength);
            }
            declared = Some(length);
        }
        Ok((response, declared))
    }

    async fn metadata(
        &self,
        url: &str,
        cap: usize,
        exact: Option<usize>,
    ) -> Result<Vec<u8>, RuntimeError> {
        let (mut response, declared) = self
            .response(url, cap as u64, exact.map(|n| n as u64))
            .await?;
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| RuntimeError::Transport)?
        {
            let length = bytes
                .len()
                .checked_add(chunk.len())
                .ok_or(RuntimeError::MetadataSize)?;
            if length > cap {
                return Err(RuntimeError::MetadataSize);
            }
            bytes.extend_from_slice(&chunk);
        }
        if exact.is_some_and(|size| bytes.len() != size) {
            return Err(RuntimeError::MetadataSize);
        }
        if declared.is_some_and(|size| bytes.len() as u64 != size) {
            return Err(RuntimeError::ContentLength);
        }
        Ok(bytes)
    }

    async fn transfer<W: AsyncWrite + Unpin>(
        &self,
        sink: &mut W,
        now: u64,
        last_sequence: LastAcceptedSequence,
    ) -> Result<CheckOutcome, RuntimeError> {
        let raw = self
            .metadata(&self.policy.manifest_url, MAX_MANIFEST_BYTES, None)
            .await?;
        let signature: [u8; 64] = self
            .metadata(&self.policy.signature_url, 64, Some(64))
            .await?
            .try_into()
            .map_err(|_| Error::InvalidSignature)?;
        let candidate = match super::verify_manifest(
            &raw,
            &signature,
            &self.policy.public_key,
            &self.policy.context(now, last_sequence),
        ) {
            Ok(candidate) => candidate,
            Err(Error::NonIncreasingSequence) => return Ok(CheckOutcome::NoUpdate),
            Err(error) => return Err(error.into()),
        };
        let manifest = candidate.manifest();
        if manifest.size > MAX_PAYLOAD_BYTES {
            return Err(RuntimeError::PayloadLimit);
        }
        let (mut response, _) = self
            .response(
                &manifest.download_url,
                MAX_PAYLOAD_BYTES,
                Some(manifest.size),
            )
            .await?;
        let mut verifier = candidate.payload_verifier();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| RuntimeError::Transport)?
        {
            verifier.update(&chunk)?;
            sink.write_all(&chunk)
                .await
                .map_err(|_| RuntimeError::Sink)?;
        }
        verifier.finalize()?;
        sink.flush().await.map_err(|_| RuntimeError::Sink)?;
        Ok(CheckOutcome::Downloaded(VerifiedTransfer {
            candidate,
            raw_manifest: raw,
            detached_signature: signature,
        }))
    }
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
// ONGROW_STORE_ADDITIONS_BEGIN
#[cfg(all(test, ongrow_update_store_probe))]
impl UpdateRuntime {
    pub(crate) fn store_probe(policy: RuntimePolicy, certificate: &[u8]) -> Result<Self, RuntimeError> {
        let certificate = reqwest::Certificate::from_pem(certificate).map_err(|_| RuntimeError::Client)?;
        let client = client_builder().add_root_certificate(certificate).build().map_err(|_| RuntimeError::Client)?;
        Ok(Self { policy, scheduler: CheckScheduler::new()?, client, deadline: CHECK_TIMEOUT })
    }
}
// ONGROW_STORE_ADDITIONS_END
