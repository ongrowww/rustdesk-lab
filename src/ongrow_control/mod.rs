use hbb_common::{
    config::Config,
    protobuf::Message as _,
    rendezvous_proto::OnGrowDeviceAttestation,
    sodiumoxide::{crypto::sign, randombytes},
    tokio,
};
use reqwest::{Client, StatusCode};
use serde_derive::Serialize;
use std::time::Duration;
use url::Url;

const ENROLLMENT_CONTEXT: &[u8] = b"ongrow-control-device-enrollment-v1";
const HEARTBEAT_CONTEXT: &[u8] = b"ongrow-control-device-heartbeat-v1";
const NONCE_BYTES: usize = 32;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, Copy, Serialize)]
pub struct Permissions {
    pub screen_recording: bool,
    pub accessibility: bool,
    pub input_monitoring: bool,
    pub audio_recording: bool,
    pub network: bool,
}

impl Permissions {
    fn bytes(self) -> [u8; 5] {
        [
            self.screen_recording as u8,
            self.accessibility as u8,
            self.input_monitoring as u8,
            self.audio_recording as u8,
            self.network as u8,
        ]
    }
}

#[derive(Serialize)]
struct EnrollmentRequest<'a> {
    device_id: &'a str,
    device_name: &'a str,
    os: &'static str,
    app_version: &'static str,
    permissions: Permissions,
    attestation: &'a str,
    request_timestamp: i64,
    request_nonce: String,
    device_signature: String,
}

#[derive(Serialize)]
struct HeartbeatRequest<'a> {
    device_id: &'a str,
    device_name: &'a str,
    os: &'static str,
    app_version: &'static str,
    permissions: Permissions,
    request_timestamp: i64,
    request_nonce: String,
    device_signature: String,
}

#[derive(Serialize)]
struct SyncResult {
    success: bool,
    enrolled: bool,
    retryable: bool,
    error: &'static str,
}

impl SyncResult {
    fn success() -> Self {
        Self {
            success: true,
            enrolled: true,
            retryable: false,
            error: "",
        }
    }

    fn error(error: &'static str, retryable: bool) -> Self {
        Self {
            success: false,
            enrolled: false,
            retryable,
            error,
        }
    }
}

#[tokio::main(flavor = "current_thread")]
pub async fn sync(enrolled: bool, permissions: Permissions) -> String {
    let result = sync_inner(enrolled, permissions)
        .await
        .unwrap_or_else(|error| error);
    serde_json::to_string(&result).unwrap_or_else(|_| {
        r#"{"success":false,"enrolled":false,"retryable":true,"error":"serialization_failed"}"#
            .to_owned()
    })
}

pub fn configured() -> bool {
    control_plane_url().is_some()
}

async fn sync_inner(
    enrolled: bool,
    permissions: Permissions,
) -> Result<SyncResult, SyncResult> {
    if crate::get_app_name() != "OnGROW Support Desk" {
        return Err(SyncResult::error("not_ongrow_client", false));
    }
    if crate::ui_interface::get_connect_status().status_num != 1 {
        return Err(SyncResult::error("offline", true));
    }

    let device_id = crate::ui_interface::get_id();
    if !is_ongrow_id(&device_id) {
        return Err(SyncResult::error("invalid_device_id", true));
    }
    let base_url = control_plane_url()
        .ok_or_else(|| SyncResult::error("control_plane_unconfigured", false))?;
    let client = Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .https_only(true)
        .build()
        .map_err(|_| SyncResult::error("http_client_unavailable", true))?;
    let device_name = crate::common::hostname();

    if enrolled {
        heartbeat(
            &client,
            &base_url,
            &device_id,
            &device_name,
            permissions,
        )
        .await
    } else {
        enroll(
            &client,
            &base_url,
            &device_id,
            &device_name,
            permissions,
        )
        .await
    }
}

async fn enroll(
    client: &Client,
    base_url: &Url,
    device_id: &str,
    device_name: &str,
    permissions: Permissions,
) -> Result<SyncResult, SyncResult> {
    let encoded_attestation = crate::ui_interface::request_ongrow_device_attestation_inner()
        .await
        .map_err(|_| SyncResult::error("attestation_failed", true))?;
    let raw_attestation = crate::decode64(&encoded_attestation)
        .map_err(|_| SyncResult::error("attestation_invalid", true))?;
    let attestation = OnGrowDeviceAttestation::parse_from_bytes(&raw_attestation)
        .map_err(|_| SyncResult::error("attestation_invalid", true))?;
    if attestation.id != device_id || attestation.nonce.len() != NONCE_BYTES {
        return Err(SyncResult::error("attestation_invalid", true));
    }

    let timestamp = hbb_common::get_time() / 1_000;
    let canonical = enrollment_canonical(
        device_id,
        device_name,
        os_name(),
        crate::VERSION,
        permissions,
        &raw_attestation,
        timestamp,
        &attestation.nonce,
    );
    let signature = sign_with_device_key(&canonical)?;
    let request = EnrollmentRequest {
        device_id,
        device_name,
        os: os_name(),
        app_version: crate::VERSION,
        permissions,
        attestation: &encoded_attestation,
        request_timestamp: timestamp,
        request_nonce: crate::encode64(&attestation.nonce),
        device_signature: crate::encode64(signature),
    };
    post(client, endpoint(base_url, "v1/device/enroll")?, &request)
        .await
        .map(|_| SyncResult::success())
}

async fn heartbeat(
    client: &Client,
    base_url: &Url,
    device_id: &str,
    device_name: &str,
    permissions: Permissions,
) -> Result<SyncResult, SyncResult> {
    hbb_common::sodiumoxide::init()
        .map_err(|_| SyncResult::error("crypto_unavailable", true))?;
    let nonce = randombytes::randombytes(NONCE_BYTES);
    let timestamp = hbb_common::get_time() / 1_000;
    let canonical = heartbeat_canonical(
        device_id,
        device_name,
        os_name(),
        crate::VERSION,
        permissions,
        timestamp,
        &nonce,
    );
    let signature = sign_with_device_key(&canonical)?;
    let request = HeartbeatRequest {
        device_id,
        device_name,
        os: os_name(),
        app_version: crate::VERSION,
        permissions,
        request_timestamp: timestamp,
        request_nonce: crate::encode64(&nonce),
        device_signature: crate::encode64(signature),
    };
    post(client, endpoint(base_url, "v1/device/heartbeat")?, &request)
        .await
        .map(|_| SyncResult::success())
}

async fn post<T: serde::Serialize>(
    client: &Client,
    endpoint: Url,
    request: &T,
) -> Result<(), SyncResult> {
    let response = client
        .post(endpoint)
        .json(request)
        .send()
        .await
        .map_err(|_| SyncResult::error("control_plane_unavailable", true))?;
    match response.status() {
        StatusCode::OK | StatusCode::CREATED => Ok(()),
        StatusCode::BAD_REQUEST | StatusCode::UNAUTHORIZED | StatusCode::CONFLICT => {
            Err(SyncResult::error("enrollment_rejected", false))
        }
        StatusCode::NOT_FOUND => Err(SyncResult::error("reenrollment_required", true)),
        StatusCode::TOO_MANY_REQUESTS | StatusCode::SERVICE_UNAVAILABLE => {
            Err(SyncResult::error("control_plane_busy", true))
        }
        _ => Err(SyncResult::error("control_plane_error", true)),
    }
}

fn sign_with_device_key(payload: &[u8]) -> Result<Vec<u8>, SyncResult> {
    hbb_common::sodiumoxide::init()
        .map_err(|_| SyncResult::error("crypto_unavailable", true))?;
    let (secret_key, _) = Config::get_key_pair();
    if secret_key.len() != sign::SECRETKEYBYTES {
        return Err(SyncResult::error("device_key_unavailable", true));
    }
    let mut bytes = [0; sign::SECRETKEYBYTES];
    bytes.copy_from_slice(&secret_key);
    Ok(sign::sign(payload, &sign::SecretKey(bytes)))
}

fn control_plane_url() -> Option<Url> {
    validate_base_url(option_env!("ONGROW_CONTROL_PLANE_URL").unwrap_or(""))
}

fn validate_base_url(raw: &str) -> Option<Url> {
    let mut url = Url::parse(raw.trim()).ok()?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    if !url.path().ends_with('/') {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }
    Some(url)
}

fn endpoint(base_url: &Url, path: &str) -> Result<Url, SyncResult> {
    base_url
        .join(path)
        .map_err(|_| SyncResult::error("control_plane_unconfigured", false))
}

fn enrollment_canonical(
    device_id: &str,
    device_name: &str,
    os: &str,
    app_version: &str,
    permissions: Permissions,
    attestation: &[u8],
    timestamp: i64,
    nonce: &[u8],
) -> Vec<u8> {
    let timestamp = timestamp.to_be_bytes();
    canonical(
        ENROLLMENT_CONTEXT,
        &[
            device_id.as_bytes(),
            device_name.as_bytes(),
            os.as_bytes(),
            app_version.as_bytes(),
            &permissions.bytes(),
            attestation,
            &timestamp,
            nonce,
        ],
    )
}

fn heartbeat_canonical(
    device_id: &str,
    device_name: &str,
    os: &str,
    app_version: &str,
    permissions: Permissions,
    timestamp: i64,
    nonce: &[u8],
) -> Vec<u8> {
    let timestamp = timestamp.to_be_bytes();
    canonical(
        HEARTBEAT_CONTEXT,
        &[
            device_id.as_bytes(),
            device_name.as_bytes(),
            os.as_bytes(),
            app_version.as_bytes(),
            &permissions.bytes(),
            &timestamp,
            nonce,
        ],
    )
}

fn canonical(context: &[u8], fields: &[&[u8]]) -> Vec<u8> {
    let mut payload = Vec::from(context);
    for field in fields {
        payload.extend_from_slice(&(field.len() as u32).to_be_bytes());
        payload.extend_from_slice(field);
    }
    payload
}

fn is_ongrow_id(value: &str) -> bool {
    value.len() == 7
        && value.starts_with("OG-")
        && value.as_bytes()[3..].iter().all(u8::is_ascii_digit)
}

#[cfg(target_os = "macos")]
fn os_name() -> &'static str {
    "macos"
}

#[cfg(target_os = "windows")]
fn os_name() -> &'static str {
    "windows"
}

#[cfg(target_os = "linux")]
fn os_name() -> &'static str {
    "linux"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_url_requires_clean_https() {
        assert!(validate_base_url("https://support.example.test/control").is_some());
        assert!(validate_base_url("http://support.example.test").is_none());
        assert!(validate_base_url("https://user@support.example.test").is_none());
        assert!(validate_base_url("https://support.example.test?token=x").is_none());
    }

    #[test]
    fn canonical_payloads_are_stable() {
        let permissions = Permissions {
            screen_recording: true,
            accessibility: false,
            input_monitoring: true,
            audio_recording: false,
            network: true,
        };
        let payload = heartbeat_canonical(
            "OG-0001",
            "test-mac",
            "macos",
            "1.4.9",
            permissions,
            1_722_345_600,
            &[9; NONCE_BYTES],
        );
        assert_eq!(
            hex::encode(payload),
            "6f6e67726f772d636f6e74726f6c2d6465766963652d6865617274626561742d7631000000074f472d3030303100000008746573742d6d6163000000056d61636f7300000005312e342e39000000050100010001000000080000000066a8e880000000200909090909090909090909090909090909090909090909090909090909090909"
        );
    }

    #[test]
    fn ongrow_id_is_strict() {
        assert!(is_ongrow_id("OG-0001"));
        assert!(!is_ongrow_id("og-0001"));
        assert!(!is_ongrow_id("OG-00001"));
        assert!(!is_ongrow_id("OG-00A1"));
    }
}
