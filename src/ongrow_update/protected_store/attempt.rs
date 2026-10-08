//! Persist exact signed bytes, not parsed installer instructions or mutable paths.
use super::*;

const MAGIC: &[u8; 8] = b"OGATT1\0\0";
const HEADER_BYTES: usize = 8 + 4 + 64;
const MAX_BYTES: usize = HEADER_BYTES + super::super::MAX_MANIFEST_BYTES;

pub(super) struct StoredAttempt { reader: File, identity: FileIdentity }

fn decode(mut reader: &File) -> Result<(Vec<u8>, [u8; 64]), Error> {
    let length = reader.metadata().map_err(|_| Error::Io)?.len();
    if length <= HEADER_BYTES as u64 || length > MAX_BYTES as u64 { return Err(Error::State); }
    reader.seek(SeekFrom::Start(0)).map_err(|_| Error::Io)?;
    let mut bytes = Vec::new();
    reader.take(MAX_BYTES as u64 + 1).read_to_end(&mut bytes).map_err(|_| Error::Io)?;
    if bytes.len() as u64 != length || bytes.get(..8) != Some(MAGIC.as_slice()) {
        return Err(Error::State);
    }
    let size = u32::from_be_bytes(bytes[8..12].try_into().map_err(|_| Error::State)?) as usize;
    if size == 0 || size > super::super::MAX_MANIFEST_BYTES || bytes.len() != HEADER_BYTES + size {
        return Err(Error::State);
    }
    let signature = bytes[12..HEADER_BYTES].try_into().map_err(|_| Error::State)?;
    Ok((bytes.split_off(HEADER_BYTES), signature))
}

impl StoredAttempt {
    fn open(context: &StoreContext) -> Result<Self, Error> {
        let reader = context.root.read(Child::Attempt)?;
        let identity = context.root.identity(&reader)?;
        Ok(Self { reader, identity })
    }
    pub(super) fn matches(&self, context: &StoreContext, raw: &[u8], signature: &[u8; 64]) -> Result<(), Error> {
        context.root.same(&self.reader, &self.identity)?;
        let (stored_raw, stored_signature) = decode(&self.reader)?;
        if stored_raw != raw || &stored_signature != signature { return Err(Error::State); }
        context.root.same(&self.reader, &self.identity)?;
        Ok(())
    }
}

pub(super) fn persist(context: &StoreContext, raw: &[u8], signature: &[u8; 64]) -> Result<StoredAttempt, Error> {
    if raw.is_empty() || raw.len() > super::super::MAX_MANIFEST_BYTES { return Err(Error::State); }
    // The payload has already been flushed and independently reverified by seal_job.
    // CREATE_NEW/O_EXCL never replaces an earlier or partially written attempt.
    let mut writer = context.root.create(Child::Attempt)?;
    let identity = context.root.identity(&writer)?;
    writer.write_all(MAGIC).map_err(|_| Error::Io)?;
    writer.write_all(&(raw.len() as u32).to_be_bytes()).map_err(|_| Error::Io)?;
    writer.write_all(signature).map_err(|_| Error::Io)?;
    writer.write_all(raw).map_err(|_| Error::Io)?;
    context.root.sync_file(&writer)?;
    context.root.sync_directory()?;
    drop(writer);
    let attempt = StoredAttempt::open(context)?;
    context.root.same(&attempt.reader, &identity)?;
    attempt.matches(context, raw, signature)?;
    Ok(attempt)
}

impl ProtectedStore {
    /// Restore a sealed download using a fresh trusted identity and current state.
    /// Neither a persisted CRC nor a previous process's verdict substitutes for
    /// signature, expiry, sequence and whole-payload verification. No network I/O.
    /// This does not acquire/clear the Pending gate or authorize an installer.
    pub(crate) async fn resume_stage(&self) -> Result<SealedStageTicket, Error> {
        let context = Arc::clone(&self.context);
        tokio::task::spawn_blocking(move || {
            context.snapshot()?;
            let lease = Arc::new(context.root.stage_lease()?);
            let snapshot = context.snapshot()?;
            let attempt = StoredAttempt::open(&context)?;
            let (raw_manifest, signature) = decode(&attempt.reader)?;
            let candidate = verify_manifest(&raw_manifest, &signature, &context.identity.key,
                &context.identity.context(context.now()?, snapshot))?;
            let reader = context.root.read(Child::Payload)?;
            let identity = context.root.identity(&reader)?;
            hash_handle(&reader, &candidate)?;
            context.root.same(&reader, &identity)?;
            attempt.matches(&context, &raw_manifest, &signature)?;
            let mut ticket = SealedStageTicket { reader, attempt, identity, _lease: lease, context,
                candidate, raw_manifest, signature };
            ticket.reverify()?;
            Ok(ticket)
        }).await.map_err(|_| Error::Io)?
    }
}
