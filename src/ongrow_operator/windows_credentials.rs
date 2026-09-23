//! Per-user Credential Manager storage for the operator's private keys.

use std::{ptr, slice};
use winapi::{
    shared::winerror::ERROR_NOT_FOUND,
    um::{
        errhandlingapi::GetLastError,
        handleapi::CloseHandle,
        synchapi::{CreateMutexW, ReleaseMutex, WaitForSingleObject},
        winbase::{WAIT_ABANDONED, WAIT_OBJECT_0},
        wincred::{
            CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_MAX_CREDENTIAL_BLOB_SIZE,
            CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
        },
    },
};
use zeroize::Zeroize;

#[cfg(not(test))]
const TARGET: &str = "de.ongrow.supportconsole.credentials/operator-v1";
#[cfg(test)]
const TARGET: &str = "de.ongrow.supportconsole.credentials/operator-v1-test";
#[cfg(not(test))]
const INIT_MUTEX: &str = "Local\\de.ongrow.supportconsole.credentials.operator-v1.init";
#[cfg(test)]
const INIT_MUTEX: &str = "Local\\de.ongrow.supportconsole.credentials.operator-v1-test.init";

pub(super) struct InitializationLock(winapi::um::winnt::HANDLE);

impl Drop for InitializationLock {
    fn drop(&mut self) {
        unsafe {
            ReleaseMutex(self.0);
            CloseHandle(self.0);
        }
    }
}

pub(super) fn lock_initialization() -> Result<InitializationLock, &'static str> {
    let name = wide(INIT_MUTEX);
    let handle = unsafe { CreateMutexW(ptr::null_mut(), 0, name.as_ptr()) };
    if handle.is_null() {
        return Err("credential_store_unavailable");
    }
    match unsafe { WaitForSingleObject(handle, 30_000) } {
        WAIT_OBJECT_0 | WAIT_ABANDONED => Ok(InitializationLock(handle)),
        _ => {
            unsafe { CloseHandle(handle) };
            Err("credential_store_unavailable")
        }
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

pub(super) fn read() -> Result<Option<Vec<u8>>, &'static str> {
    let target = wide(TARGET);
    let mut credential = ptr::null_mut();
    // Credential Manager scopes generic credentials to the current Windows user.
    if unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut credential) } == 0 {
        return if unsafe { GetLastError() } == ERROR_NOT_FOUND {
            Ok(None)
        } else {
            Err("credential_store_unavailable")
        };
    }
    if credential.is_null() {
        return Err("credential_store_unavailable");
    }
    let record = unsafe { &*credential };
    let length = record.CredentialBlobSize as usize;
    let result = if length == 0
        || length > CRED_MAX_CREDENTIAL_BLOB_SIZE as usize
        || record.CredentialBlob.is_null()
    {
        Err("invalid_keychain_record")
    } else {
        Ok(Some(unsafe {
            slice::from_raw_parts(record.CredentialBlob, length).to_vec()
        }))
    };
    if !record.CredentialBlob.is_null() && length <= CRED_MAX_CREDENTIAL_BLOB_SIZE as usize {
        unsafe { slice::from_raw_parts_mut(record.CredentialBlob, length) }.zeroize();
    }
    unsafe { CredFree(credential.cast()) };
    result
}

pub(super) fn write(raw: &mut [u8]) -> Result<(), &'static str> {
    if raw.is_empty() || raw.len() > CRED_MAX_CREDENTIAL_BLOB_SIZE as usize {
        return Err("credential_record_too_large");
    }
    let mut target = wide(TARGET);
    let mut username = wide(super::KEYCHAIN_ACCOUNT);
    let mut record: CREDENTIALW = unsafe { std::mem::zeroed() };
    record.Type = CRED_TYPE_GENERIC;
    record.TargetName = target.as_mut_ptr();
    record.UserName = username.as_mut_ptr();
    record.CredentialBlobSize = raw.len() as u32;
    record.CredentialBlob = raw.as_mut_ptr();
    // Despite its API name, LOCAL_MACHINE persistence remains user-scoped.
    record.Persist = CRED_PERSIST_LOCAL_MACHINE;
    if unsafe { CredWriteW(&mut record, 0) } == 0 {
        Err("credential_store_unavailable")
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winapi::um::wincred::CredDeleteW;

    #[test]
    fn isolated_credential_roundtrip_update_and_parallel_initialization() {
        let target = wide(TARGET);
        unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) };
        assert_eq!(read().unwrap(), None);
        let workers: Vec<_> = (0..8)
            .map(|_| {
                std::thread::spawn(|| {
                    let _guard = lock_initialization().unwrap();
                    if read().unwrap().is_none() {
                        write(&mut b"test-record-v1".to_vec()).unwrap();
                    }
                    read().unwrap()
                })
            })
            .collect();
        for worker in workers {
            assert_eq!(worker.join().unwrap(), Some(b"test-record-v1".to_vec()));
        }
        let mut replacement = b"test-record-v2".to_vec();
        write(&mut replacement).unwrap();
        assert_eq!(read().unwrap(), Some(replacement));
        assert_eq!(
            write(&mut vec![0; CRED_MAX_CREDENTIAL_BLOB_SIZE as usize + 1]),
            Err("credential_record_too_large")
        );
        unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) };
    }
}
