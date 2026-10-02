use std::{
    collections::{BTreeSet, HashMap},
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

pub const PRODUCT: &str = "OnGROW Support Desk";
pub const DESK_EXE: &str = "OnGROW Support Desk.exe";
pub const BROKER_EXE: &str = "RuntimeBroker_ongrow_support_desk.exe";
const CACHE_PREFIX: &str = "OnGROW-Support-Desk-Setup-";

pub fn install_args(args: &[String]) -> Result<Vec<String>, String> {
    match args {
        [] => Ok(vec!["--install".into()]),
        [arg] if arg == "--install" => Ok(args.to_vec()),
        _ => Err("Setup accepts only --install or --ongrow-verify-payload".into()),
    }
}

// Windows rules are checked on every host, including the packaging host.
pub fn relative_path(input: &str) -> Result<String, String> {
    let path = input.replace('\\', "/");
    let path = path.strip_prefix("./").unwrap_or(&path);
    if path.is_empty() || path.starts_with('/') || path.contains(':') || !path.is_ascii() {
        return Err("Invalid Windows payload path".into());
    }
    for component in path.split('/') {
        if component.is_empty()
            || component == "."
            || component == ".."
            || component.ends_with(['.', ' '])
            || component.bytes().any(|c| c < 32 || b"<>\"|?*".contains(&c))
        {
            return Err("Unsafe Windows payload path component".into());
        }
        let base = component
            .split('.')
            .next()
            .unwrap_or("")
            .to_ascii_uppercase();
        if ["CON", "PRN", "AUX", "NUL", "CLOCK$", "CONIN$", "CONOUT$"].contains(&base.as_str())
            || (base.len() == 4
                && (base.starts_with("COM") || base.starts_with("LPT"))
                && matches!(base.as_bytes()[3], b'1'..=b'9'))
        {
            return Err("Reserved Windows payload name".into());
        }
    }
    Ok(path.to_owned())
}

fn redirected(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            // FILE_ATTRIBUTE_REPARSE_POINT
            return true;
        }
    }
    false
}

pub fn checked_directory(path: &Path) -> Result<(), String> {
    if !path.is_absolute() {
        return Err("Setup directory must be absolute".into());
    }
    for ancestor in path.ancestors() {
        let metadata =
            fs::symlink_metadata(ancestor).map_err(|e| format!("Directory check failed: {e}"))?;
        if redirected(&metadata) || !metadata.is_dir() {
            return Err("Redirected or non-directory setup path".into());
        }
    }
    Ok(())
}

pub struct OwnedCache {
    root: PathBuf,
    identity: fs::Metadata,
    files: HashMap<PathBuf, Vec<u8>>,
    directories: BTreeSet<PathBuf>,
}

impl OwnedCache {
    pub fn create(root: PathBuf) -> Result<Self, String> {
        let name = root
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or("Invalid setup cache name")?;
        if !name.starts_with(CACHE_PREFIX) {
            return Err("Foreign setup cache refused".into());
        }
        let parent = root.parent().ok_or("Setup cache has no parent")?;
        checked_directory(parent)?;
        // create_dir, never create_dir_all or removal of a pre-existing cache.
        fs::create_dir(&root).map_err(|e| format!("Cannot create exclusive setup cache: {e}"))?;
        checked_directory(&root)?;
        let identity = fs::symlink_metadata(&root)
            .map_err(|e| format!("Cannot record cache ownership: {e}"))?;
        Ok(Self {
            root,
            identity,
            files: HashMap::new(),
            directories: BTreeSet::new(),
        })
    }

    fn verify_ownership(&self) -> Result<(), String> {
        checked_directory(&self.root)?;
        let current = fs::symlink_metadata(&self.root)
            .map_err(|e| format!("Cannot verify cache ownership: {e}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if (current.dev(), current.ino()) != (self.identity.dev(), self.identity.ino()) {
                return Err("Replaced setup cache retained".into());
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if current.creation_time() != self.identity.creation_time() {
                return Err("Replaced setup cache retained".into());
            }
        }
        Ok(())
    }

    pub fn write(&mut self, relative: &str, bytes: &[u8]) -> Result<(), String> {
        let relative = relative_path(relative)?;
        self.verify_ownership()?;
        let path = self.root.join(&relative);
        let parent = path.parent().ok_or("Payload file has no parent")?;
        let mut current = self.root.clone();
        checked_directory(&current)?;
        for component in parent
            .strip_prefix(&self.root)
            .map_err(|_| "Payload escaped cache")?
            .components()
        {
            current.push(component);
            if !self.directories.contains(&current) {
                fs::create_dir(&current)
                    .map_err(|e| format!("Cannot create payload directory: {e}"))?;
                self.directories.insert(current.clone());
            }
            checked_directory(&current)?;
        }
        checked_directory(parent)?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| format!("Cannot create payload file: {e}"))?;
        output
            .write_all(bytes)
            .map_err(|e| format!("Cannot write payload file: {e}"))?;
        output
            .sync_all()
            .map_err(|e| format!("Cannot flush payload file: {e}"))?;
        if fs::read(&path).map_err(|e| format!("Cannot verify extracted file: {e}"))? != bytes {
            return Err("Extracted file verification failed".into());
        }
        self.files.insert(path, bytes.to_vec());
        Ok(())
    }

    fn verify_tree(&self, directory: &Path) -> Result<(), String> {
        checked_directory(directory)?;
        for entry in
            fs::read_dir(directory).map_err(|e| format!("Cannot inspect setup cache: {e}"))?
        {
            let entry = entry.map_err(|e| format!("Cannot inspect setup cache entry: {e}"))?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)
                .map_err(|e| format!("Cannot inspect cache file: {e}"))?;
            if redirected(&metadata) {
                return Err("Redirected setup cache retained".into());
            }
            if metadata.is_dir() && self.directories.contains(&path) {
                self.verify_tree(&path)?;
            } else if metadata.is_file() && self.files.contains_key(&path) {
                if fs::read(&path).map_err(|e| format!("Cannot verify cache file: {e}"))?
                    != self.files[&path]
                {
                    return Err("Changed setup cache file retained".into());
                }
            } else {
                return Err("Unknown setup cache entry retained".into());
            }
        }
        Ok(())
    }

    pub fn cleanup(self) -> Result<(), String> {
        // Check the entire tree before deleting anything. Unknown leftovers stay.
        self.verify_ownership()?;
        self.verify_tree(&self.root)?;
        for (path, expected) in &self.files {
            if fs::read(path).map_err(|e| format!("Owned payload missing: {e}"))? != *expected {
                return Err("Changed owned payload retained".into());
            }
        }
        for path in self.files.keys() {
            checked_directory(path.parent().ok_or("Cache file has no parent")?)?;
            let metadata =
                fs::symlink_metadata(path).map_err(|e| format!("Missing owned file: {e}"))?;
            if redirected(&metadata) || !metadata.is_file() {
                return Err("Changed cache target retained".into());
            }
            fs::remove_file(path).map_err(|e| format!("Cannot remove owned payload file: {e}"))?;
        }
        let mut directories: Vec<_> = self.directories.into_iter().collect();
        directories.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
        for directory in directories {
            checked_directory(&directory)?;
            fs::remove_dir(directory)
                .map_err(|e| format!("Cannot remove owned payload directory: {e}"))?;
        }
        checked_directory(&self.root)?;
        fs::remove_dir(self.root).map_err(|e| format!("Cannot remove owned setup cache: {e}"))
    }
}

#[cfg(feature = "ongrow-support-desk")]
pub fn run() -> Result<(), String> {
    use crate::bin_reader::checked;
    let args: Vec<_> = std::env::args().skip(1).collect();
    let verify_only = args == ["--ongrow-verify-payload"];
    let child_args = if verify_only {
        Vec::new()
    } else {
        install_args(&args)?
    };
    let files = checked::parse(checked::BIN_DATA)?;
    #[cfg(not(windows))]
    {
        let _ = (child_args, files);
        return Err("OnGROW Setup runs only on Windows".into());
    }
    #[cfg(windows)]
    {
        use std::{
            process::Command,
            time::{SystemTime, UNIX_EPOCH},
        };
        let local = std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA is missing")?;
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| format!("Cannot create cache identity: {e}"))?
            .as_nanos();
        let root =
            PathBuf::from(local).join(format!("{CACHE_PREFIX}{}-{nonce}", std::process::id()));
        let mut cache = OwnedCache::create(root)?;
        let file_count = files.len();
        for file in files {
            // Failure leaves the exclusive cache intact, never deletes unknown data.
            cache.write(&file.path, &file.bytes)?;
        }
        if verify_only {
            cache.cleanup()?;
            println!("{PRODUCT} Setup payload extracted and verified; {file_count} files; no client started");
            return Ok(());
        }
        use std::os::windows::ffi::OsStringExt;
        let mut buffer = [0u16; 32768];
        let length = unsafe {
            windows::Win32::System::SystemInformation::GetSystemDirectoryW(Some(&mut buffer))
        } as usize;
        if length == 0 || length >= buffer.len() {
            return Err("Cannot locate Windows system directory".into());
        }
        let broker_source = PathBuf::from(std::ffi::OsString::from_wide(&buffer[..length]))
            .join("RuntimeBroker.exe");
        checked_directory(broker_source.parent().ok_or("Broker parent missing")?)?;
        let metadata = fs::symlink_metadata(&broker_source)
            .map_err(|e| format!("Cannot inspect broker: {e}"))?;
        if redirected(&metadata) || !metadata.is_file() {
            return Err("Redirected broker source refused".into());
        }
        let broker =
            fs::read(&broker_source).map_err(|e| format!("Cannot copy product broker: {e}"))?;
        cache.write(BROKER_EXE, &broker)?;
        cache.verify_tree(&cache.root)?;
        // Never derive configuration, quick support or product identity from the wrapper filename.
        let status = Command::new(cache.root.join(DESK_EXE))
            .args(child_args)
            .current_dir(&cache.root)
            .env("RUSTDESK_APPNAME", DESK_EXE)
            .env("SET_FOREGROUND_WINDOW", "1")
            .spawn()
            .map_err(|e| format!("Cannot start installation dialog: {e}"))?
            .wait()
            .map_err(|e| format!("Cannot wait for installation dialog: {e}"))?;
        cache.cleanup()?;
        if !status.success() {
            return Err("Installation dialog exited unsuccessfully".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn root() -> PathBuf {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        std::env::temp_dir().canonicalize().unwrap().join(format!(
            "{CACHE_PREFIX}test-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn identity_and_install_args_do_not_depend_on_wrapper_name() {
        assert_eq!(PRODUCT, "OnGROW Support Desk");
        assert_eq!(DESK_EXE, "OnGROW Support Desk.exe");
        assert_eq!(BROKER_EXE, "RuntimeBroker_ongrow_support_desk.exe");
        for _filename in [
            "renamed.exe",
            "no-install.exe",
            "rustdesk-qs.exe",
            "password=secret.exe",
        ] {
            assert_eq!(install_args(&[]).unwrap(), ["--install"]);
        }
        assert!(install_args(&["--silent-install".into()]).is_err());
        assert!(install_args(&["--password".into(), "secret".into()]).is_err());
    }

    #[test]
    fn foreign_and_existing_cache_are_never_cleared() {
        let path = root();
        fs::create_dir(&path).unwrap();
        fs::write(path.join("foreign"), b"keep").unwrap();
        assert!(OwnedCache::create(path.clone()).is_err());
        assert_eq!(fs::read(path.join("foreign")).unwrap(), b"keep");
        assert!(OwnedCache::create(path.parent().unwrap().join("rustdesk")).is_err());
        fs::remove_file(path.join("foreign")).unwrap();
        fs::remove_dir(path).unwrap();
    }

    #[test]
    fn cleanup_only_removes_verified_owned_payload() {
        let path = root();
        let mut cache = OwnedCache::create(path.clone()).unwrap();
        cache
            .write("data/flutter_assets/nested/file", b"asset")
            .unwrap();
        cache.write(DESK_EXE, b"exe").unwrap();
        cache.cleanup().unwrap();
        assert!(!path.exists());
        let path = root();
        let mut cache = OwnedCache::create(path.clone()).unwrap();
        cache.write(DESK_EXE, b"exe").unwrap();
        fs::write(path.join("unknown"), b"keep").unwrap();
        assert!(cache.cleanup().is_err());
        assert!(path.join(DESK_EXE).exists());
        fs::remove_file(path.join("unknown")).unwrap();
        fs::remove_file(path.join(DESK_EXE)).unwrap();
        fs::remove_dir(path).unwrap();
    }

    #[test]
    fn changed_files_and_write_failures_are_reported() {
        let path = root();
        let mut cache = OwnedCache::create(path.clone()).unwrap();
        cache.write(DESK_EXE, b"exe").unwrap();
        assert!(cache.write(DESK_EXE, b"other").is_err());
        fs::write(path.join(DESK_EXE), b"changed").unwrap();
        assert!(cache.cleanup().is_err());
        assert_eq!(fs::read(path.join(DESK_EXE)).unwrap(), b"changed");
        fs::remove_file(path.join(DESK_EXE)).unwrap();
        fs::remove_dir(path).unwrap();
    }

    #[test]
    fn replaced_cache_is_never_cleaned() {
        let path = root();
        let cache = OwnedCache::create(path.clone()).unwrap();
        let original = root();
        fs::rename(&path, &original).unwrap();
        fs::create_dir(&path).unwrap();
        fs::write(path.join("foreign"), b"keep").unwrap();
        assert!(cache.cleanup().is_err());
        assert_eq!(fs::read(path.join("foreign")).unwrap(), b"keep");
        fs::remove_file(path.join("foreign")).unwrap();
        fs::remove_dir(path).unwrap();
        fs::remove_dir(original).unwrap();
    }

    #[test]
    fn windows_path_rules_are_host_independent() {
        assert_eq!(relative_path(".\\data\\a").unwrap(), "data/a");
        for path in [
            "",
            "../a",
            "././a",
            "C:a",
            "/a",
            "//server/share",
            "a//b",
            "a:stream",
            "CON",
            "Lpt9.txt",
            "a.",
            "a ",
            "a/NUL.txt",
            "a/../b",
            "a/*",
        ] {
            assert!(relative_path(path).is_err(), "{path}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn symlink_cache_and_parents_are_refused() {
        use std::os::unix::fs::symlink;
        let actual = root();
        fs::create_dir(&actual).unwrap();
        let link = root();
        symlink(&actual, &link).unwrap();
        assert!(OwnedCache::create(link.clone()).is_err());
        assert!(OwnedCache::create(link.join(format!("{CACHE_PREFIX}child"))).is_err());
        let mut cache = OwnedCache::create(root()).unwrap();
        symlink(&actual, cache.root.join("data")).unwrap();
        assert!(cache.write("data/file", b"escape").is_err());
        assert!(!actual.join("file").exists());
        let cache_root = cache.root.clone();
        assert!(cache.cleanup().is_err());
        fs::remove_file(cache_root.join("data")).unwrap();
        fs::remove_dir(cache_root).unwrap();
        fs::remove_file(link).unwrap();
        fs::remove_dir(actual).unwrap();
    }
}
