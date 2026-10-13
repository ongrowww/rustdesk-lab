//! Installer authority is derived from the signed ticket under the real gate.
use super::super::windows_integration::HealthyService;
use super::*;
use std::path::PathBuf;

impl SealedStageTicket {
    pub(crate) fn manifest(&self) -> &super::super::manifest::Manifest {
        self.candidate.manifest()
    }
    pub(super) fn recovery_context<'a>(
        &'a self,
        mut context: Context<'a>,
    ) -> Result<Context<'a>, Error> {
        // Only resume_stage_recovery, which requires a held Pending Transaction,
        // constructs recovery tickets. Signature/context/time remain unchanged.
        context.baked_current_sequence = 0;
        context.last_accepted_sequence = LastAcceptedSequence::Known(0);
        Ok(context)
    }
    pub(super) fn require_recovery_sequence(
        &self,
        candidate: &VerifiedCandidate,
    ) -> Result<(), Error> {
        let accepted = match self.context.snapshot()?.state {
            LastAcceptedSequence::Known(s) => s,
            _ => return Err(Error::State),
        };
        if candidate.manifest().release_sequence < accepted
            || candidate.manifest().release_sequence < self.context.identity.baked_sequence
        {
            return Err(Error::State);
        }
        Ok(())
    }
    pub(crate) fn installer_path(&mut self) -> Result<PathBuf, Error> {
        self.reverify()?;
        if self.manifest().release_sequence > u32::MAX as u64
            || !self.manifest().filename.ends_with(".msi")
        {
            return Err(Error::State);
        }
        let root = &self.context.root;
        let reader = match root.read(Child::Installer) {
            Ok(reader) => reader,
            Err(session_gate::Error::MissingGate) => {
                let mut source = &self.reader;
                source.seek(SeekFrom::Start(0)).map_err(|_| Error::Io)?;
                let mut writer = root.create(Child::Installer)?;
                let identity = root.identity(&writer)?;
                std::io::copy(&mut source, &mut writer).map_err(|_| Error::Io)?;
                root.sync_file(&writer)?;
                drop(writer);
                let reader = root.read(Child::Installer)?;
                root.same(&reader, &identity)?;
                reader
            }
            Err(error) => return Err(error.into()),
        };
        hash_handle(&reader, &self.candidate)?;
        // Structured-storage header, with the original signed manifest still held.
        let mut header = [0; 8];
        let mut source = &reader;
        source.seek(SeekFrom::Start(0)).map_err(|_| Error::Io)?;
        source.read_exact(&mut header).map_err(|_| Error::Io)?;
        if header != [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1] {
            return Err(Error::State);
        }
        self.installer = Some(reader);
        Ok(root.path(Child::Installer))
    }
}
impl ProtectedStore {
    /// Safe scratch cleanup only while a real Ready/shared admission lease and
    /// the independent stage lock exclude every legitimate native installer.
    pub(crate) fn discard_ready(&self, ready: &session_gate::SessionLease) -> Result<(), Error> {
        if !ready.protects_ready() {
            return Err(Error::State);
        }
        self.snapshot()?;
        let _lease = self.context.root.stage_lease()?;
        self.cleanup_slots()
    }

    pub(crate) async fn resume_stage_recovery(
        &self,
        _transaction: &session_gate::Transaction,
    ) -> Result<SealedStageTicket, Error> {
        let context = Arc::clone(&self.context);
        tokio::task::spawn_blocking(move || {
            context.snapshot()?;
            let lease = Arc::new(context.root.stage_lease()?);
            let attempt = attempt::StoredAttempt::open(&context)?;
            let (raw_manifest, signature) = attempt::decode(&attempt.reader)?;
            let mut verify_context = context
                .identity
                .context(context.now()?, context.snapshot()?);
            verify_context.baked_current_sequence = 0;
            verify_context.last_accepted_sequence = LastAcceptedSequence::Known(0);
            let candidate = verify_manifest(
                &raw_manifest,
                &signature,
                &context.identity.key,
                &verify_context,
            )?;
            let reader = context.root.read(Child::Payload)?;
            let identity = context.root.identity(&reader)?;
            let mut ticket = SealedStageTicket {
                reader,
                attempt,
                identity,
                _lease: lease,
                context,
                candidate,
                raw_manifest,
                signature,
                recovery: true,
                installer: None,
            };
            ticket.reverify()?;
            Ok(ticket)
        })
        .await
        .map_err(|_| Error::Io)?
    }
    pub(crate) fn commit_windows(
        &self,
        mut ticket: SealedStageTicket,
        proof: &HealthyService,
    ) -> Result<(), Error> {
        ticket.reverify()?;
        if !Arc::ptr_eq(&self.context, &ticket.context)
            || ticket.manifest().release_sequence != proof.sequence
        {
            return Err(Error::State);
        }
        let context = Arc::clone(&ticket.context);
        let lease = Arc::clone(&ticket._lease);
        // This is after real new-service health, and before admission opens.
        context
            .root
            .replace(Child::Sequence, &context.identity.record(proof.sequence))?;
        drop(ticket);
        self.cleanup_slots()?;
        drop(lease);
        Ok(())
    }
    pub(crate) fn rollback_windows(
        &self,
        ticket: SealedStageTicket,
        proof: &HealthyService,
    ) -> Result<(), Error> {
        if !Arc::ptr_eq(&self.context, &ticket.context)
            || proof.sequence != self.context.identity.baked_sequence
            || !matches!(self.snapshot()?.state,LastAcceptedSequence::Known(s) if s==proof.sequence)
        {
            return Err(Error::State);
        }
        let lease = Arc::clone(&ticket._lease);
        drop(ticket);
        self.cleanup_slots()?;
        drop(lease);
        Ok(())
    }
    fn cleanup_slots(&self) -> Result<(), Error> {
        for child in [
            Child::Installer,
            Child::Payload,
            Child::Attempt,
            Child::NativeNext,
            Child::NativeState,
        ] {
            match self.context.root.read(child) {
                Ok(file) => {
                    drop(file);
                    self.context.root.remove(child)?;
                }
                Err(session_gate::Error::MissingGate) => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }
    pub(crate) fn cleanup_committed(&self, proof: &HealthyService) -> Result<(), Error> {
        if !matches!(self.snapshot()?.state, LastAcceptedSequence::Known(s) if s==proof.sequence) {
            return Err(Error::State);
        }
        let _lease = self.context.root.stage_lease()?;
        self.cleanup_slots()
    }
}
