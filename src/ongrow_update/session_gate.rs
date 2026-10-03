//! Process-independent admission barrier. Off until protected bootstrap exists.
//!
//! A session owns a shared OS lock through its last I/O operation. Installation
//! owns the exclusive lock and durably records Pending before stopping anything.
//! Dropping an installer never clears Pending. Only verified health may do that.
use std::{fmt, sync::Arc};

#[cfg(target_os = "macos")]
#[path = "session_gate/macos.rs"]
mod os;
#[cfg(target_os = "windows")]
#[path = "session_gate/windows.rs"]
mod os;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Busy,
    Pending,
    MissingGate,
    Untrusted,
    Io,
    Disabled,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "OnGROW session admission: {:?}", self)
    }
}
impl std::error::Error for Error {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Product {
    CustomerDesk,
    SupportConsole,
}

// Build-time opt-in only. Runtime environment and mutable client configuration
// cannot enable or disable this policy. Old Lab builds continue to work.
fn startup_policy() -> Result<Option<Product>, Error> {
    match option_env!("ONGROW_UPDATE_SESSION_GATE") {
        None => return Ok(None),
        Some("v1") => {},
        Some(_) => return Err(Error::Untrusted),
    }
    match option_env!("ONGROW_PRODUCT_ROLE") {
        Some("customer-desk") => Ok(Some(Product::CustomerDesk)),
        Some("support-console") => Ok(Some(Product::SupportConsole)),
        _ => Err(Error::Untrusted),
    }
}

/// Clones share the same kernel-lock handle. The last clone releases the lock.
/// No Rust mutex, PID, timer or process liveness check substitutes for that lock.
#[derive(Clone)]
pub struct SessionLease {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    _lock: Option<Arc<os::Lock>>,
}

pub fn admit() -> Result<SessionLease, Error> {
    let product = startup_policy()?;
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        let lock = match product {
            Some(product) => Some(Arc::new(os::admit(product)?)),
            None => None,
        };
        Ok(SessionLease { _lock: lock })
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = product;
        Ok(SessionLease {})
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub struct Transaction {
    lock: os::Lock,
}

/// The future authenticated privileged bootstrap calls this explicitly.
/// Session admission never creates, repairs or resets protected state.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) fn initialize() -> Result<(), Error> {
    os::initialize(startup_policy()?.ok_or(Error::Disabled)?)
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) fn begin_installation() -> Result<Transaction, Error> {
    let mut lock = os::exclusive(startup_policy()?.ok_or(Error::Disabled)?)?;
    lock.mark_pending()?;
    Ok(Transaction { lock })
}

// No production constructor exists yet. Session users cannot declare arbitrary
// processes healthy or clear a crash journal. The installer/recovery milestone
// must provide the authenticated health proof before adding a constructor.
pub(crate) struct VerifiedHealth(());

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl Transaction {
    pub(crate) fn commit_healthy(mut self, _health: VerifiedHealth) -> Result<(), Error> {
        self.lock.mark_ready()
    }
}

#[cfg(test)]
#[path = "session_gate_tests.rs"]
mod tests;
// ONGROW_STORE_ADDITIONS_BEGIN
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) mod store_handles {
    use super::{os, Error, Product};
    use std::fs::File;

    #[derive(Clone, Copy)]
    pub(crate) enum Child { Sequence, StageLock, Payload }
    impl Child {
        pub(super) fn name(self) -> &'static str {
            match self {
                Self::Sequence => "accepted-sequence-v1",
                Self::StageLock => "staging-v1.lock",
                Self::Payload => "stage-v1.payload",
            }
        }
    }

    pub(crate) struct ProtectedRoot { inner: os::ProtectedRoot }
    pub(crate) struct FileIdentity(os::StoreFileIdentity);
    pub(crate) struct StageLease { _inner: os::StoreLock }
    impl ProtectedRoot {
        pub(crate) fn open(product: Product) -> Result<Self, Error> {
            Ok(Self { inner: os::ProtectedRoot::open(product)? })
        }
        pub(crate) fn create(&self, child: Child) -> Result<File, Error> { self.inner.create(child) }
        pub(crate) fn read(&self, child: Child) -> Result<File, Error> { self.inner.read(child) }
        pub(crate) fn identity(&self, file: &File) -> Result<FileIdentity, Error> {
            Ok(FileIdentity(self.inner.identity(file)?))
        }
        pub(crate) fn same(&self, file: &File, identity: &FileIdentity) -> Result<(), Error> {
            if self.inner.identity(file)? != identity.0 { return Err(Error::Untrusted); }
            Ok(())
        }
        pub(crate) fn sync_file(&self, file: &File) -> Result<(), Error> { self.inner.sync_file(file) }
        pub(crate) fn sync_directory(&self) -> Result<(), Error> { self.inner.sync_directory() }
        pub(crate) fn stage_lease(&self) -> Result<StageLease, Error> {
            Ok(StageLease { _inner: self.inner.stage_lease()? })
        }
        #[cfg(all(test, ongrow_update_store_probe))]
        pub(crate) fn fixture(path: &std::path::Path, product: Product) -> Result<Self, Error> {
            if product != Product::SupportConsole { return Err(Error::Untrusted); }
            Ok(Self { inner: os::ProtectedRoot::fixture(path)? })
        }
    }
    #[cfg(all(test, ongrow_update_store_probe))]
    pub(crate) fn fixture_gate_initialize(path: &std::path::Path) -> Result<(), Error> { os::fixture_initialize(path) }
    #[cfg(all(test, ongrow_update_store_probe))]
    pub(crate) fn fixture_session(path: &std::path::Path) -> Result<super::SessionLease, Error> {
        Ok(super::SessionLease { _lock: Some(std::sync::Arc::new(os::fixture_acquire(path, false)?)) })
    }
}
// ONGROW_STORE_ADDITIONS_END
