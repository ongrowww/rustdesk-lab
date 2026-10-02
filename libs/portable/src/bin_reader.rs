#[cfg(not(feature = "ongrow-support-desk"))]
pub(crate) use upstream::BinaryReader;

#[cfg(not(feature = "ongrow-support-desk"))]
mod upstream {
use std::{
    fs::{self},
    io::{Cursor, Read},
    path::Path,
};

#[cfg(windows)]
const BIN_DATA: &[u8] = include_bytes!("../data.bin");
#[cfg(not(windows))]
const BIN_DATA: &[u8] = &[];
// 4bytes
const LENGTH: usize = 4;
const IDENTIFIER_LENGTH: usize = 8;
const MD5_LENGTH: usize = 32;
const BUF_SIZE: usize = 4096;

pub(crate) struct BinaryData {
    pub md5_code: &'static [u8],
    // compressed gzip data
    pub raw: &'static [u8],
    pub path: String,
}

pub(crate) struct BinaryReader {
    pub files: Vec<BinaryData>,
    pub exe: String,
}

impl Default for BinaryReader {
    fn default() -> Self {
        let (files, exe) = BinaryReader::read();
        Self { files, exe }
    }
}

impl BinaryData {
    fn decompress(&self) -> Vec<u8> {
        let cursor = Cursor::new(self.raw);
        let mut decoder = brotli::Decompressor::new(cursor, BUF_SIZE);
        let mut buf = Vec::new();
        decoder.read_to_end(&mut buf).ok();
        buf
    }

    pub fn write_to_file(&self, prefix: &Path) {
        let p = prefix.join(&self.path);
        if let Some(parent) = p.parent() {
            if !parent.exists() {
                let _ = fs::create_dir_all(parent);
            }
        }
        if p.exists() {
            // check md5
            let f = fs::read(p.clone()).unwrap_or_default();
            let digest = format!("{:x}", md5::compute(&f));
            let md5_record = String::from_utf8_lossy(self.md5_code);
            if digest == md5_record {
                // same, skip this file
                println!("skip {}", &self.path);
                return;
            } else {
                println!("writing {}", p.display());
                println!("{} -> {}", md5_record, digest)
            }
        }
        let _ = fs::write(p, self.decompress());
    }
}

impl BinaryReader {
    fn read() -> (Vec<BinaryData>, String) {
        let mut base: usize = 0;
        let mut parsed = vec![];
        assert!(BIN_DATA.len() > IDENTIFIER_LENGTH, "bin data invalid!");
        let mut iden = String::from_utf8_lossy(&BIN_DATA[base..base + IDENTIFIER_LENGTH]);
        if iden != "rustdesk" {
            panic!("bin file is not valid!");
        }
        base += IDENTIFIER_LENGTH;
        loop {
            iden = String::from_utf8_lossy(&BIN_DATA[base..base + IDENTIFIER_LENGTH]);
            if iden == "rustdesk" {
                base += IDENTIFIER_LENGTH;
                break;
            }
            // start reading
            let mut offset = 0;
            let path_length = u32::from_be_bytes([
                BIN_DATA[base + offset],
                BIN_DATA[base + offset + 1],
                BIN_DATA[base + offset + 2],
                BIN_DATA[base + offset + 3],
            ]) as usize;
            offset += LENGTH;
            let path =
                String::from_utf8_lossy(&BIN_DATA[base + offset..base + offset + path_length])
                    .to_string();
            offset += path_length;
            // file sz
            let file_length = u32::from_be_bytes([
                BIN_DATA[base + offset],
                BIN_DATA[base + offset + 1],
                BIN_DATA[base + offset + 2],
                BIN_DATA[base + offset + 3],
            ]) as usize;
            offset += LENGTH;
            let raw = &BIN_DATA[base + offset..base + offset + file_length];
            offset += file_length;
            // md5
            let md5 = &BIN_DATA[base + offset..base + offset + MD5_LENGTH];
            offset += MD5_LENGTH;
            parsed.push(BinaryData {
                md5_code: md5,
                raw: raw,
                path: path,
            });
            base += offset;
        }
        // executable
        let executable = String::from_utf8_lossy(&BIN_DATA[base..]).to_string();
        (parsed, executable)
    }

    #[cfg(linux)]
    pub fn configure_permission(&self, prefix: &Path) {
        use std::os::unix::prelude::PermissionsExt;

        let exe_path = prefix.join(&self.exe);
        if exe_path.exists() {
            if let Ok(f) = File::open(exe_path) {
                if let Ok(meta) = f.metadata() {
                    let mut permissions = meta.permissions();
                    permissions.set_mode(0o755);
                    f.set_permissions(permissions).ok();
                }
            }
        }
    }
}

}

#[cfg(feature = "ongrow-support-desk")]
pub(crate) mod checked {
    use std::{collections::HashSet, io::{Cursor, Read}};
    use crate::ongrow_setup::{relative_path, DESK_EXE};

    #[cfg(all(windows, not(test)))]
    pub const BIN_DATA: &[u8] = include_bytes!("../data.bin");
    #[cfg(any(not(windows), test))]
    pub const BIN_DATA: &[u8] = &[];

    pub struct File {
        pub path: String,
        pub bytes: Vec<u8>,
    }

    struct CursorReader<'a> {
        remaining: &'a [u8],
    }

    impl<'a> CursorReader<'a> {
        fn take(&mut self, length: usize) -> Result<&'a [u8], String> {
            if length > self.remaining.len() {
                return Err("Truncated payload".into());
            }
            let (head, tail) = self.remaining.split_at(length);
            self.remaining = tail;
            Ok(head)
        }

        fn length(&mut self) -> Result<usize, String> {
            let bytes = self.take(4)?;
            Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize)
        }

        fn text(&mut self, length: usize) -> Result<String, String> {
            std::str::from_utf8(self.take(length)?)
                .map(str::to_owned).map_err(|_| "Invalid payload UTF-8".into())
        }
    }

    // MD5 checks the existing embed format's consistency. It is not a signature.
    pub fn parse(data: &[u8]) -> Result<Vec<File>, String> {
        let mut reader = CursorReader { remaining: data };
        if reader.take(8)? != b"rustdesk" {
            return Err("Invalid payload identifier".into());
        }
        let mut names = HashSet::new();
        let mut files = Vec::new();
        loop {
            if reader.remaining.starts_with(b"rustdesk") {
                reader.take(8)?;
                break;
            }
            let length = reader.length()?;
            let path = relative_path(&reader.text(length)?)?;
            let key = path.to_lowercase();
            if !names.insert(key) {
                return Err("Duplicate payload path".into());
            }
            let length = reader.length()?;
            let compressed = reader.take(length)?;
            let expected = reader.text(32)?;
            if !expected.bytes().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()) {
                return Err("Invalid payload MD5".into());
            }
            let mut bytes = Vec::new();
            brotli::Decompressor::new(Cursor::new(compressed), 4096)
                .read_to_end(&mut bytes).map_err(|_| "Payload decompression failed".to_owned())?;
            if format!("{:x}", md5::compute(&bytes)) != expected {
                return Err("Payload MD5 mismatch".into());
            }
            files.push(File { path, bytes });
        }
        let length = reader.remaining.len();
        let executable = relative_path(&reader.text(length)?)?;
        if executable != DESK_EXE || !files.iter().any(|f| f.path == DESK_EXE && !f.bytes.is_empty()) {
            return Err("Expected customer Desk executable is missing".into());
        }
        for required in ["librustdesk.dll", "flutter_windows.dll", "data/icudtl.dat", "data/flutter_assets/AssetManifest.json", "LICENCE", "ongrow-build-provenance.txt"] {
            if !files.iter().any(|f| f.path == required && !f.bytes.is_empty()) {
                return Err(format!("Required payload file missing: {required}"));
            }
        }
        for file in &files {
            let lower = file.path.to_lowercase();
            if lower == "rustdesk.exe" || lower == "ongrow support console.exe" || lower == "runtimebroker_rustdesk.exe" {
                return Err("Foreign product in payload".into());
            }
            let parts: Vec<_> = lower.split('/').collect();
            for count in 1..parts.len() {
                if names.contains(&parts[..count].join("/")) {
                    return Err("Payload file/directory collision".into());
                }
            }
        }
        Ok(files)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::io::Write;

        fn payload(entries: &[(&str, &[u8])], executable: &str) -> Vec<u8> {
            let mut data = b"rustdesk".to_vec();
            for (path, bytes) in entries {
                let mut compressed = Vec::new();
                {
                    let mut writer = brotli::CompressorWriter::new(&mut compressed, 4096, 3, 22);
                    writer.write_all(bytes).unwrap();
                }
                data.extend_from_slice(&(path.len() as u32).to_be_bytes());
                data.extend_from_slice(path.as_bytes());
                data.extend_from_slice(&(compressed.len() as u32).to_be_bytes());
                data.extend_from_slice(&compressed);
                data.extend_from_slice(format!("{:x}", md5::compute(bytes)).as_bytes());
            }
            data.extend_from_slice(b"rustdesk");
            data.extend_from_slice(executable.as_bytes());
            data
        }

        fn entries() -> Vec<(&'static str, &'static [u8])> {
            vec![(DESK_EXE, b"MZ"), ("librustdesk.dll", b"core"), ("flutter_windows.dll", b"flutter"),
                ("data/icudtl.dat", b"icu"), ("data/flutter_assets/AssetManifest.json", b"{}"),
                ("LICENCE", b"license"), ("ongrow-build-provenance.txt", b"source=lab")]
        }

        #[test]
        fn real_nested_payload_and_generator_prefix() {
            let mut entries = entries();
            entries.push((".\\data\\flutter_assets\\nested\\example", b"nested bytes"));
            let files = parse(&payload(&entries, "./OnGROW Support Desk.exe")).unwrap();
            assert_eq!(files.last().unwrap().bytes, b"nested bytes");
            assert_eq!(files.last().unwrap().path, "data/flutter_assets/nested/example");
            let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
                "OnGROW-Support-Desk-Setup-nested-{}-{}", std::process::id(),
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
            let mut cache = crate::ongrow_setup::OwnedCache::create(root.clone()).unwrap();
            for file in files {
                cache.write(&file.path, &file.bytes).unwrap();
            }
            assert_eq!(std::fs::read(root.join("data/flutter_assets/nested/example")).unwrap(), b"nested bytes");
            cache.cleanup().unwrap();
            assert!(!root.exists());
        }

        #[test]
        fn every_truncation_is_rejected() {
            let data = payload(&entries(), DESK_EXE);
            for length in 0..data.len() {
                assert!(parse(&data[..length]).is_err(), "length {length}");
            }
        }

        #[test]
        fn compression_hash_paths_and_missing_runtime_fail() {
            let data = payload(&entries(), DESK_EXE);
            let offset = 8 + 4 + DESK_EXE.len() + 4;
            let mut corrupt = data.clone();
            corrupt[offset] = 0xff;
            assert!(parse(&corrupt).is_err());
            let mut corrupt = data;
            let hash_offset = offset + u32::from_be_bytes(corrupt[offset-4..offset].try_into().unwrap()) as usize;
            corrupt[hash_offset] = if corrupt[hash_offset] == b'0' { b'1' } else { b'0' };
            assert!(parse(&corrupt).is_err());
            for path in ["../escape", "C:/escape", "\\\\host\\share", "data/CON.txt", "data/a:stream", "data/escape.", "data/../escape"] {
                let mut entries = entries();
                entries.push((path, b"bad"));
                assert!(parse(&payload(&entries, DESK_EXE)).is_err(), "{path}");
            }
            for required in 0..entries().len() {
                let mut entries = entries();
                entries.remove(required);
                assert!(parse(&payload(&entries, DESK_EXE)).is_err());
            }
            let mut entries = entries();
            entries.push(("DATA/ICUDTL.DAT", b"duplicate"));
            assert!(parse(&payload(&entries, DESK_EXE)).is_err());
            assert!(parse(&payload(&entries, "rustdesk.exe")).is_err());
        }
    }
}
