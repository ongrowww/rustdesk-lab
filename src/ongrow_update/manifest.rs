use serde_derive::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Product {
    CustomerDesk,
    SupportConsole,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Platform {
    #[serde(rename = "windows-x64")]
    WindowsX64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Channel {
    Lab,
    Stable,
}

/// Schema 1 uses integer Unix seconds and lowercase hexadecimal hashes.
/// Deriving Deserialize for a struct rejects duplicate fields, including
/// escaped spellings of the same JSON key. Unknown fields are forbidden.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u32,
    pub product: Product,
    pub platform: Platform,
    pub channel: Channel,
    pub release_sequence: u64,
    pub upstream_version: String,
    pub source_sha: String,
    pub issued_at: u64,
    pub expires_at: u64,
    pub filename: String,
    pub size: u64,
    pub sha256: String,
    pub download_url: String,
}
