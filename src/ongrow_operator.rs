use hbb_common::{
    sha2::{Digest, Sha256},
    sodiumoxide::{
        base64::{self, Variant},
        crypto::{box_, sealedbox, sign},
        randombytes,
    },
};
use reqwest::{blocking::Client, StatusCode};
use serde_derive::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};
use url::Url;
use zeroize::Zeroize;

const APP_NAME: &str = "OnGROW Support Console";
const KEYCHAIN_SERVICE: &str = "de.ongrow.supportconsole.credentials";
const KEYCHAIN_ACCOUNT: &str = "operator-v1";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const PENDING_LAUNCH_TTL: Duration = Duration::from_secs(60);
const TICKET_BYTES: usize = 32;
const NONCE_BYTES: usize = 32;
const ENVELOPE_VERSION: u8 = 1;
const GENERATION_BYTES: usize = 16;
const MIN_PASSWORD_BYTES: usize = 16;
const MAX_PASSWORD_BYTES: usize = 128;
const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

#[derive(Clone, Default, Serialize)]
struct OperatorStatus {
    state: String,
    error: String,
    console_id: String,
    signing_public_key: String,
    encryption_public_key: String,
    signing_key_fingerprint: String,
    encryption_key_fingerprint: String,
    control_plane_url: String,
}

#[derive(Serialize, Deserialize)]
struct StoredCredentials {
    version: u8,
    console_id: String,
    signing_public_key: String,
    signing_secret_key: String,
    encryption_public_key: String,
    encryption_secret_key: String,
}

struct Credentials {
    console_id: String,
    signing_public_key: sign::PublicKey,
    signing_secret_key: sign::SecretKey,
    encryption_public_key: box_::PublicKey,
    encryption_secret_key: box_::SecretKey,
}

impl Drop for Credentials {
    fn drop(&mut self) {
        self.signing_secret_key.0.zeroize();
        self.encryption_secret_key.0.zeroize();
    }
}

struct PendingLaunch {
    device_id: String,
    password: String,
    launch_id: String,
    console_id: String,
    created_at: Instant,
}

impl Drop for PendingLaunch {
    fn drop(&mut self) {
        self.password.zeroize();
    }
}

#[derive(Serialize)]
struct LaunchForFlutter {
    device_id: String,
    handle: String,
}

#[derive(Serialize)]
struct NativeProofRequest<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    ticket: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    launch_id: Option<&'a str>,
    console_id: &'a str,
    timestamp: i64,
    nonce: String,
    signature: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConsoleClaimResponse {
    console_id: String,
    status: String,
    signing_key_fingerprint: String,
    encryption_key_fingerprint: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RedemptionResponse {
    launch_id: String,
    device_id: String,
    algorithm: String,
    format_version: u8,
    envelope: String,
}

lazy_static::lazy_static! {
    static ref STATUS: Mutex<OperatorStatus> = Mutex::new(OperatorStatus::default());
    static ref PENDING: Mutex<HashMap<String, PendingLaunch>> = Mutex::new(HashMap::new());
    static ref READY_HANDLE: Mutex<String> = Mutex::new(String::new());
}

pub fn status() -> String {
    {
        let status = STATUS.lock().unwrap();
        if !status.state.is_empty() {
            return serde_json::to_string(&*status).unwrap_or_else(|_| {
                "{\"state\":\"failed\",\"error\":\"serialization_failed\"}".to_owned()
            });
        }
    }
    let next = match credentials() {
        Ok(credentials) => public_status(&credentials),
        Err(error) => OperatorStatus {
            state: "failed".to_owned(),
            error: error.to_owned(),
            ..OperatorStatus::default()
        },
    };
    let mut status = STATUS.lock().unwrap();
    *status = next;
    serde_json::to_string(&*status)
        .unwrap_or_else(|_| "{\"state\":\"failed\",\"error\":\"serialization_failed\"}".to_owned())
}

pub fn handle_uri(raw: String) -> bool {
    if crate::get_app_name() != APP_NAME {
        return false;
    }
    let Ok(uri) = Url::parse(&raw) else {
        if raw.starts_with("ongrow-support-console:") {
            set_error("invalid_deep_link");
            return true;
        }
        return false;
    };
    if uri.scheme() != "ongrow-support-console" {
        return false;
    }
    if uri.username() != ""
        || uri.password().is_some()
        || uri.port().is_some()
        || uri.query().is_some()
        || uri.fragment().is_some()
    {
        set_error("invalid_deep_link");
        return true;
    }
    let Some(authority) = uri.host_str() else {
        set_error("invalid_deep_link");
        return true;
    };
    let value = uri.path().strip_prefix('/').unwrap_or("");
    if value.is_empty() || value.contains('/') {
        set_error("invalid_deep_link");
        return true;
    }
    match authority {
        "registered" if valid_opaque(value, 16, 64) => {
            spawn_claim(value.to_owned());
        }
        "launch" if valid_ticket(value) => {
            spawn_redeem(value.to_owned());
        }
        _ => set_error("invalid_deep_link"),
    }
    true
}

pub fn take_launch() -> String {
    let mut status = STATUS.lock().unwrap();
    if status.state != "ready" {
        return String::new();
    }
    let handle = READY_HANDLE.lock().unwrap().clone();
    let device_id = {
        let mut pending = PENDING.lock().unwrap();
        pending.retain(|_, launch| launch.created_at.elapsed() <= PENDING_LAUNCH_TTL);
        pending.get(&handle).map(|launch| launch.device_id.clone())
    };
    let Some(device_id) = device_id else {
        status.state = "failed".to_owned();
        status.error = "launch_expired".to_owned();
        return String::new();
    };
    status.state = "opening".to_owned();
    READY_HANDLE.lock().unwrap().clear();
    serde_json::to_string(&LaunchForFlutter { device_id, handle }).unwrap_or_default()
}

pub fn consume_launch(
    handle: &str,
    device_id: &str,
) -> Result<(String, String, String), &'static str> {
    let mut launch = PENDING
        .lock()
        .unwrap()
        .remove(handle)
        .ok_or("launch_expired")?;
    if launch.created_at.elapsed() > PENDING_LAUNCH_TTL {
        return Err("launch_expired");
    }
    if launch.device_id != device_id {
        return Err("launch_device_mismatch");
    }
    Ok((
        std::mem::take(&mut launch.password),
        std::mem::take(&mut launch.launch_id),
        std::mem::take(&mut launch.console_id),
    ))
}

pub fn acknowledge_async(launch_id: String, console_id: String) {
    std::thread::spawn(move || {
        if acknowledge(&launch_id, &console_id).is_ok() {
            set_idle();
        } else {
            set_error("acknowledgement_failed");
        }
    });
}

fn public_status(credentials: &Credentials) -> OperatorStatus {
    OperatorStatus {
        state: if credentials.console_id.is_empty() {
            "registration_required"
        } else {
            "idle"
        }
        .to_owned(),
        error: String::new(),
        console_id: credentials.console_id.clone(),
        signing_public_key: encode(credentials.signing_public_key.as_ref()),
        encryption_public_key: encode(credentials.encryption_public_key.as_ref()),
        signing_key_fingerprint: fingerprint(credentials.signing_public_key.as_ref()),
        encryption_key_fingerprint: fingerprint(credentials.encryption_public_key.as_ref()),
        control_plane_url: validated_base_url().unwrap_or_default(),
    }
}

fn spawn_claim(console_id: String) {
    if !begin_operation("registering") {
        return;
    }
    std::thread::spawn(move || match claim(&console_id) {
        Ok(()) => set_idle(),
        Err(error) => set_error(error),
    });
}

fn spawn_redeem(ticket: String) {
    if !begin_operation("redeeming") {
        return;
    }
    std::thread::spawn(move || match redeem(&ticket) {
        Ok((handle, device_id)) => {
            *READY_HANDLE.lock().unwrap() = handle;
            let mut status = STATUS.lock().unwrap();
            status.state = "ready".to_owned();
            status.error.clear();
            let _ = device_id;
        }
        Err(error) => set_error(error),
    });
}

fn claim(console_id: &str) -> Result<(), &'static str> {
    let mut credentials = credentials()?;
    let timestamp = chrono::Utc::now().timestamp();
    let nonce = randombytes::randombytes(NONCE_BYTES);
    let canonical = canonical_claim(console_id, timestamp, &nonce)?;
    let signature = sign::sign_detached(canonical.as_bytes(), &credentials.signing_secret_key);
    let request = NativeProofRequest {
        ticket: None,
        launch_id: None,
        console_id,
        timestamp,
        nonce: encode(&nonce),
        signature: encode(signature.as_ref()),
    };
    let response = client()?
        .post(endpoint("/v1/native/support-consoles/claim")?)
        .json(&request)
        .send()
        .map_err(|_| "control_plane_unavailable")?;
    if response.status() != StatusCode::OK {
        return Err("console_claim_rejected");
    }
    let result: ConsoleClaimResponse = response.json().map_err(|_| "invalid_control_response")?;
    if result.console_id != console_id
        || result.status != "active"
        || result.signing_key_fingerprint != fingerprint(credentials.signing_public_key.as_ref())
        || result.encryption_key_fingerprint
            != fingerprint(credentials.encryption_public_key.as_ref())
    {
        return Err("console_claim_mismatch");
    }
    credentials.console_id = console_id.to_owned();
    store_credentials(&credentials)
}

fn redeem(ticket: &str) -> Result<(String, String), &'static str> {
    let credentials = credentials()?;
    if credentials.console_id.is_empty() {
        return Err("console_not_registered");
    }
    let timestamp = chrono::Utc::now().timestamp();
    let nonce = randombytes::randombytes(NONCE_BYTES);
    let canonical = canonical_redeem(&credentials.console_id, ticket, timestamp, &nonce)?;
    let signature = sign::sign_detached(canonical.as_bytes(), &credentials.signing_secret_key);
    let request = NativeProofRequest {
        ticket: Some(ticket),
        launch_id: None,
        console_id: &credentials.console_id,
        timestamp,
        nonce: encode(&nonce),
        signature: encode(signature.as_ref()),
    };
    let response = client()?
        .post(endpoint("/v1/native/support-launch/redeem")?)
        .json(&request)
        .send()
        .map_err(|_| "control_plane_unavailable")?;
    if response.status() != StatusCode::OK {
        return Err("launch_rejected");
    }
    let result: RedemptionResponse = response.json().map_err(|_| "invalid_control_response")?;
    if result.algorithm != "curve25519-sealed-box"
        || result.format_version != 1
        || !valid_opaque(&result.launch_id, 16, 64)
    {
        return Err("invalid_control_response");
    }
    let ciphertext = decode(&result.envelope).map_err(|_| "invalid_control_response")?;
    let mut envelope = sealedbox::open(
        &ciphertext,
        &credentials.encryption_public_key,
        &credentials.encryption_secret_key,
    )
    .map_err(|_| "secret_open_failed")?;
    let password = parse_envelope(&envelope)?;
    envelope.zeroize();
    let handle = hex::encode(randombytes::randombytes(24));
    let mut pending = PENDING.lock().unwrap();
    pending.clear();
    pending.insert(
        handle.clone(),
        PendingLaunch {
            device_id: result.device_id.clone(),
            password,
            launch_id: result.launch_id,
            console_id: credentials.console_id.clone(),
            created_at: Instant::now(),
        },
    );
    Ok((handle, result.device_id))
}

fn acknowledge(launch_id: &str, console_id: &str) -> Result<(), &'static str> {
    let credentials = credentials()?;
    if credentials.console_id != console_id {
        return Err("console_mismatch");
    }
    let timestamp = chrono::Utc::now().timestamp();
    let nonce = randombytes::randombytes(NONCE_BYTES);
    let canonical = canonical_ack(console_id, launch_id, timestamp, &nonce)?;
    let signature = sign::sign_detached(canonical.as_bytes(), &credentials.signing_secret_key);
    let request = NativeProofRequest {
        ticket: None,
        launch_id: Some(launch_id),
        console_id,
        timestamp,
        nonce: encode(&nonce),
        signature: encode(signature.as_ref()),
    };
    let response = client()?
        .post(endpoint("/v1/native/support-launch/ack")?)
        .json(&request)
        .send()
        .map_err(|_| "control_plane_unavailable")?;
    if response.status() == StatusCode::NO_CONTENT {
        Ok(())
    } else {
        Err("acknowledgement_rejected")
    }
}

fn parse_envelope(raw: &[u8]) -> Result<String, &'static str> {
    if raw.len() < 19 || raw[0] != ENVELOPE_VERSION {
        return Err("invalid_secret_envelope");
    }
    let length = u16::from_be_bytes([raw[17], raw[18]]) as usize;
    if !(MIN_PASSWORD_BYTES..=MAX_PASSWORD_BYTES).contains(&length) || raw.len() != 19 + length {
        return Err("invalid_secret_envelope");
    }
    let _generation = &raw[1..1 + GENERATION_BYTES];
    String::from_utf8(raw[19..].to_vec()).map_err(|_| "invalid_secret_envelope")
}

fn credentials() -> Result<Credentials, &'static str> {
    if crate::get_app_name() != APP_NAME {
        return Err("wrong_product_role");
    }
    hbb_common::sodiumoxide::init().map_err(|_| "crypto_unavailable")?;
    #[cfg(target_os = "macos")]
    {
        use security_framework::passwords::get_generic_password;
        match get_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT) {
            Ok(mut raw) => {
                let parsed = decode_credentials(&raw);
                raw.zeroize();
                parsed
            }
            Err(error) if error.code() == ERR_SEC_ITEM_NOT_FOUND => create_credentials(),
            Err(_) => Err("keychain_unavailable"),
        }
    }
    #[cfg(not(target_os = "macos"))]
    Err("platform_not_supported")
}

fn create_credentials() -> Result<Credentials, &'static str> {
    let (signing_public_key, signing_secret_key) = sign::gen_keypair();
    let (encryption_public_key, encryption_secret_key) = box_::gen_keypair();
    let credentials = Credentials {
        console_id: String::new(),
        signing_public_key,
        signing_secret_key,
        encryption_public_key,
        encryption_secret_key,
    };
    store_credentials(&credentials)?;
    Ok(credentials)
}

fn decode_credentials(raw: &[u8]) -> Result<Credentials, &'static str> {
    let stored: StoredCredentials =
        serde_json::from_slice(raw).map_err(|_| "invalid_keychain_record")?;
    if stored.version != 1
        || (!stored.console_id.is_empty() && !valid_opaque(&stored.console_id, 16, 64))
    {
        return Err("invalid_keychain_record");
    }
    let signing_public_key = fixed_key::<{ sign::PUBLICKEYBYTES }>(&stored.signing_public_key)?;
    let mut signing_secret_key = fixed_key::<{ sign::SECRETKEYBYTES }>(&stored.signing_secret_key)?;
    let encryption_public_key =
        fixed_key::<{ box_::PUBLICKEYBYTES }>(&stored.encryption_public_key)?;
    let mut encryption_secret_key =
        fixed_key::<{ box_::SECRETKEYBYTES }>(&stored.encryption_secret_key)?;
    let credentials = Credentials {
        console_id: stored.console_id,
        signing_public_key: sign::PublicKey(signing_public_key),
        signing_secret_key: sign::SecretKey(signing_secret_key),
        encryption_public_key: box_::PublicKey(encryption_public_key),
        encryption_secret_key: box_::SecretKey(encryption_secret_key),
    };
    signing_secret_key.zeroize();
    encryption_secret_key.zeroize();
    Ok(credentials)
}

fn store_credentials(credentials: &Credentials) -> Result<(), &'static str> {
    let stored = StoredCredentials {
        version: 1,
        console_id: credentials.console_id.clone(),
        signing_public_key: encode(credentials.signing_public_key.as_ref()),
        signing_secret_key: encode(credentials.signing_secret_key.as_ref()),
        encryption_public_key: encode(credentials.encryption_public_key.as_ref()),
        encryption_secret_key: encode(credentials.encryption_secret_key.as_ref()),
    };
    let mut raw = serde_json::to_vec(&stored).map_err(|_| "keychain_serialization_failed")?;
    #[cfg(target_os = "macos")]
    let result = security_framework::passwords::set_generic_password(
        KEYCHAIN_SERVICE,
        KEYCHAIN_ACCOUNT,
        &raw,
    )
    .map_err(|_| "keychain_unavailable");
    #[cfg(not(target_os = "macos"))]
    let result = Err("platform_not_supported");
    raw.zeroize();
    result
}

fn fixed_key<const N: usize>(encoded: &str) -> Result<[u8; N], &'static str> {
    let mut decoded = decode(encoded).map_err(|_| "invalid_keychain_record")?;
    if decoded.len() != N {
        decoded.zeroize();
        return Err("invalid_keychain_record");
    }
    let mut value = [0; N];
    value.copy_from_slice(&decoded);
    decoded.zeroize();
    Ok(value)
}

fn client() -> Result<Client, &'static str> {
    Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|_| "http_client_unavailable")
}

fn endpoint(path: &str) -> Result<String, &'static str> {
    Ok(format!("{}{}", validated_base_url()?, path))
}

fn validated_base_url() -> Result<String, &'static str> {
    let raw = option_env!("ONGROW_CONTROL_PLANE_URL").unwrap_or("");
    let parsed = Url::parse(raw).map_err(|_| "control_plane_not_configured")?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none()
        || parsed.username() != ""
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err("control_plane_not_configured");
    }
    Ok(raw.trim_end_matches('/').to_owned())
}

fn canonical_claim(console_id: &str, timestamp: i64, nonce: &[u8]) -> Result<String, &'static str> {
    Ok(format!(
        "ongrow-support-console-claim-v1\n{}\n{}\n{}",
        console_id,
        rfc3339(timestamp)?,
        encode(nonce)
    ))
}

fn canonical_redeem(
    console_id: &str,
    ticket: &str,
    timestamp: i64,
    nonce: &[u8],
) -> Result<String, &'static str> {
    Ok(format!(
        "ongrow-support-launch-redeem-v1\n{}\n{}\n{}\n{}",
        console_id,
        ticket,
        rfc3339(timestamp)?,
        encode(nonce)
    ))
}

fn canonical_ack(
    console_id: &str,
    launch_id: &str,
    timestamp: i64,
    nonce: &[u8],
) -> Result<String, &'static str> {
    Ok(format!(
        "ongrow-support-launch-ack-v1\n{}\n{}\n{}\n{}",
        console_id,
        launch_id,
        rfc3339(timestamp)?,
        encode(nonce)
    ))
}

fn rfc3339(timestamp: i64) -> Result<String, &'static str> {
    use chrono::TimeZone;
    chrono::Utc
        .timestamp_opt(timestamp, 0)
        .single()
        .map(|value| value.format("%Y-%m-%dT%H:%M:%SZ").to_string())
        .ok_or("invalid_timestamp")
}

fn encode(raw: &[u8]) -> String {
    base64::encode(raw, Variant::Original)
}

fn decode(encoded: &str) -> Result<Vec<u8>, ()> {
    base64::decode(encoded, Variant::Original).map_err(|_| ())
}

fn fingerprint(raw: &[u8]) -> String {
    hex::encode(Sha256::digest(raw))
}

fn valid_ticket(value: &str) -> bool {
    value.len() == 43
        && base64::decode(value, Variant::UrlSafeNoPadding)
            .map(|raw| raw.len() == TICKET_BYTES)
            .unwrap_or(false)
}

fn valid_opaque(value: &str, min: usize, max: usize) -> bool {
    if value.len() < min || value.len() > max {
        return false;
    }
    base64::decode(value, Variant::UrlSafeNoPadding)
        .map(|raw| (16..=64).contains(&raw.len()))
        .unwrap_or(false)
}

fn set_state(state: &str, error: &str) {
    let mut status = STATUS.lock().unwrap();
    status.state = state.to_owned();
    status.error = error.to_owned();
}

fn begin_operation(state: &str) -> bool {
    let mut status = STATUS.lock().unwrap();
    if matches!(
        status.state.as_str(),
        "registering" | "redeeming" | "ready" | "opening"
    ) {
        return false;
    }
    status.state = state.to_owned();
    status.error.clear();
    true
}

fn set_error(error: &'static str) {
    set_state("failed", error);
}

fn set_idle() {
    if let Ok(credentials) = credentials() {
        *STATUS.lock().unwrap() = public_status(&credentials);
    } else {
        set_error("keychain_unavailable");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deep_links_reject_parameters_and_passwords() {
        assert!(!handle_uri("https://example.test/launch/value".to_owned()));
        assert!(handle_uri(
            "ongrow-support-console://launch/value?password=secret".to_owned()
        ));
        assert_eq!(STATUS.lock().unwrap().error, "invalid_deep_link");
    }

    #[test]
    fn parses_password_envelope() {
        let password = b"abcdefghijklmnop";
        let mut envelope = vec![ENVELOPE_VERSION];
        envelope.extend_from_slice(&[7; GENERATION_BYTES]);
        envelope.extend_from_slice(&(password.len() as u16).to_be_bytes());
        envelope.extend_from_slice(password);
        assert_eq!(parse_envelope(&envelope), Ok("abcdefghijklmnop".to_owned()));
        envelope.push(0);
        assert_eq!(parse_envelope(&envelope), Err("invalid_secret_envelope"));
    }
}
