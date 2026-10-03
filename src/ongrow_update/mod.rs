//! Signed verification and bounded transport. No app caller, persistence or apply.
#![allow(dead_code)] // Not connected to an updater until trusted bootstrap exists.

pub mod manifest;
pub mod session_gate;
pub(crate) mod runtime;
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) mod protected_store;

use hbb_common::sodiumoxide::{self, crypto::sign};
use manifest::{Channel, Manifest, Platform, Product};
use sha2::{Digest, Sha256};

pub const MAX_MANIFEST_BYTES: usize = 64 * 1024;
pub const SIGNATURE_DOMAIN: &[u8] = b"OnGROW signed update manifest v1\0";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    UpstreamDisabled,
    ManifestTooLarge,
    InvalidKey,
    InvalidSignature,
    CryptoUnavailable,
    InvalidManifest,
    WrongContext,
    InvalidTime,
    UnknownReleaseState,
    NonIncreasingSequence,
    InvalidFilename,
    InvalidDownloadLocation,
    PayloadSize,
    PayloadHash,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "OnGROW update rejected: {:?}", self)
    }
}

impl std::error::Error for Error {}

/// Matches the product names applied by scripts/apply_ongrow_product_profile.py.
/// The additional compile-time role also keeps a renamed build fail-closed.
pub fn upstream_allowed(app_name: &str, build_role: Option<&str>) -> Result<(), Error> {
    if matches!(app_name, "OnGROW Support Desk" | "OnGROW Support Console")
        || matches!(build_role, Some("customer-desk" | "support-console"))
    {
        Err(Error::UpstreamDisabled)
    } else {
        Ok(())
    }
}

pub(crate) fn require_upstream_allowed() -> Result<(), Error> {
    initialize_product_policy();
    upstream_allowed(&crate::get_app_name(), option_env!("ONGROW_PRODUCT_ROLE"))?;
    PRODUCT_POLICY.require_upstream_allowed()
}

#[derive(Default)]
struct ProductPolicy {
    upstream_disabled: std::sync::OnceLock<bool>,
}

impl ProductPolicy {
    fn initialize(&self, app_name: &str, build_role: Option<&str>) {
        self.upstream_disabled
            .get_or_init(|| upstream_allowed(app_name, build_role).is_err());
    }

    fn require_upstream_allowed(&self) -> Result<(), Error> {
        if *self.upstream_disabled.get().unwrap_or(&true) {
            Err(Error::UpstreamDisabled)
        } else {
            Ok(())
        }
    }
}

static PRODUCT_POLICY: ProductPolicy = ProductPolicy {
    upstream_disabled: std::sync::OnceLock::new(),
};

/// Capture the baked product identity before custom-client branding can mutate
/// APP_NAME. Called at the main entry and before custom configuration is read.
pub(crate) fn initialize_product_policy() {
    PRODUCT_POLICY.initialize(&crate::get_app_name(), option_env!("ONGROW_PRODUCT_ROLE"));
}

/// Match core_main's first command after the four startup-only flags.
pub(crate) fn is_update_command(args: impl IntoIterator<Item = impl AsRef<str>>) -> bool {
    args.into_iter()
        .find(|arg| {
            !matches!(
                arg.as_ref(),
                "--elevate" | "--run-as-system" | "--quick_support" | "--no-server"
            )
        })
        .is_some_and(|arg| arg.as_ref() == "--update")
}

#[derive(Debug, Clone, Copy)]
pub enum LastAcceptedSequence {
    /// Supplied by a future protected state provider, not ordinary user config.
    Known(u64),
    /// Missing or corrupt state must not enable an older release.
    Unknown,
}

/// Caller-owned trust context. No manifest field can choose these values.
pub struct Context<'a> {
    pub product: Product,
    pub platform: Platform,
    pub channel: Channel,
    pub now_unix_seconds: u64,
    pub baked_current_sequence: u64,
    pub last_accepted_sequence: LastAcceptedSequence,
    pub download_location: &'a DownloadLocation,
}

/// One locally trusted HTTPS origin and slash-terminated path prefix.
/// This module deliberately supplies no production origin or trust key.
pub struct DownloadLocation {
    origin: String,
    path_prefix: String,
}

impl DownloadLocation {
    pub fn new(origin: &str, path_prefix: &str) -> Result<Self, Error> {
        let url = url::Url::parse(origin).map_err(|_| Error::InvalidDownloadLocation)?;
        if !origin.starts_with("https://")
            || !origin.is_ascii()
            || origin.contains(['%', '\\', '?', '#', '@'])
            || url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.path() != "/"
            || url.origin().ascii_serialization() != origin
            || !path_prefix.starts_with('/')
            || path_prefix.starts_with("//")
            || !path_prefix.ends_with('/')
            || path_prefix == "/"
            || !plain_path(path_prefix.trim_matches('/'))
        {
            return Err(Error::InvalidDownloadLocation);
        }
        Ok(Self {
            origin: origin.to_owned(),
            path_prefix: path_prefix.to_owned(),
        })
    }

    fn permits(&self, raw: &str, filename: &str) -> bool {
        // Reject ambiguous encodings before URL parsing can normalize them.
        if !raw.is_ascii() || raw.contains(['%', '\\', '?', '#', '@']) {
            return false;
        }
        let Ok(url) = url::Url::parse(raw) else {
            return false;
        };
        if url.scheme() != "https"
            || url.origin().ascii_serialization() != self.origin
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.as_str() != raw
        {
            return false;
        }
        let Some(path) = raw.strip_prefix(&self.origin) else {
            return false;
        };
        path.starts_with(&self.path_prefix)
            && plain_path(path.trim_start_matches('/'))
            && path.rsplit('/').next() == Some(filename)
    }
}

fn plain_path(path: &str) -> bool {
    path.split('/').all(|part| {
        !part.is_empty()
            && part != "."
            && part != ".."
            && part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    })
}

fn plain_filename(filename: &str) -> bool {
    if filename.is_empty()
        || filename.len() > 255
        || !plain_path(filename)
        || filename.contains('/')
        || filename.ends_with('.')
    {
        return false;
    }
    let base = filename
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    !matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !(base.len() == 4
            && (base.starts_with("COM") || base.starts_with("LPT"))
            && matches!(base.as_bytes()[3], b'1'..=b'9'))
}

fn lowercase_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
}

/// Only this function can create a candidate. It is data, never a command.
#[derive(Debug)]
pub struct VerifiedCandidate {
    manifest: Manifest,
}

impl VerifiedCandidate {
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    pub fn payload_verifier(&self) -> StreamingPayloadVerifier {
        StreamingPayloadVerifier {
            expected_size: self.manifest.size,
            expected_hash: self.manifest.sha256.clone(),
            received: 0,
            hash: Sha256::new(),
        }
    }

    /// Uses the same verifier as the streaming transport.
    pub fn verify_payload(&self, payload: &[u8]) -> Result<(), Error> {
        let mut verifier = self.payload_verifier();
        verifier.update(payload)?;
        verifier.finalize()
    }
}

/// Size is checked before hashing, and before the caller writes a chunk.
pub struct StreamingPayloadVerifier {
    expected_size: u64,
    expected_hash: String,
    received: u64,
    hash: Sha256,
}

impl StreamingPayloadVerifier {
    pub fn update(&mut self, chunk: &[u8]) -> Result<(), Error> {
        let length = u64::try_from(chunk.len()).map_err(|_| Error::PayloadSize)?;
        let total = self.received.checked_add(length).ok_or(Error::PayloadSize)?;
        if total > self.expected_size {
            return Err(Error::PayloadSize);
        }
        self.hash.update(chunk);
        self.received = total;
        Ok(())
    }

    pub fn finalize(self) -> Result<(), Error> {
        if self.received != self.expected_size {
            return Err(Error::PayloadSize);
        }
        if format!("{:x}", self.hash.finalize()) != self.expected_hash {
            return Err(Error::PayloadHash);
        }
        Ok(())
    }
}

pub fn verify_manifest(
    raw: &[u8],
    detached_signature: &[u8],
    trusted_public_key: &[u8],
    context: &Context<'_>,
) -> Result<VerifiedCandidate, Error> {
    if raw.len() > MAX_MANIFEST_BYTES {
        return Err(Error::ManifestTooLarge);
    }
    let key = sign::PublicKey::from_slice(trusted_public_key).ok_or(Error::InvalidKey)?;
    // from_bytes enforces exactly 64 bytes. No padding or attached signature.
    let signature =
        sign::Signature::from_bytes(detached_signature).map_err(|_| Error::InvalidSignature)?;
    sodiumoxide::init().map_err(|_| Error::CryptoUnavailable)?;
    let mut message = Vec::with_capacity(SIGNATURE_DOMAIN.len() + raw.len());
    message.extend_from_slice(SIGNATURE_DOMAIN);
    message.extend_from_slice(raw);
    if !sign::verify_detached(&signature, &message, &key) {
        return Err(Error::InvalidSignature);
    }
    // Authenticate the exact bytes before any JSON interpretation.
    let manifest: Manifest = serde_json::from_slice(raw).map_err(|_| Error::InvalidManifest)?;
    if manifest.schema_version != 1
        || !lowercase_hex(&manifest.source_sha, 40)
        || !lowercase_hex(&manifest.sha256, 64)
        || manifest.upstream_version.is_empty()
        || manifest.upstream_version.len() > 64
        || !manifest
            .upstream_version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
        || manifest.size == 0
    {
        return Err(Error::InvalidManifest);
    }
    if manifest.product != context.product
        || manifest.platform != context.platform
        || manifest.channel != context.channel
    {
        return Err(Error::WrongContext);
    }
    if manifest.issued_at > context.now_unix_seconds
        || manifest.expires_at <= context.now_unix_seconds
        || manifest.expires_at <= manifest.issued_at
    {
        return Err(Error::InvalidTime);
    }
    let last = match context.last_accepted_sequence {
        LastAcceptedSequence::Known(sequence) => sequence,
        LastAcceptedSequence::Unknown => return Err(Error::UnknownReleaseState),
    };
    if manifest.release_sequence <= context.baked_current_sequence
        || manifest.release_sequence <= last
    {
        return Err(Error::NonIncreasingSequence);
    }
    if !plain_filename(&manifest.filename) {
        return Err(Error::InvalidFilename);
    }
    if !context
        .download_location
        .permits(&manifest.download_url, &manifest.filename)
    {
        return Err(Error::InvalidDownloadLocation);
    }
    Ok(VerifiedCandidate { manifest })
}

#[cfg(test)]
mod tests;
