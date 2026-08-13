use hbb_common::{
    config::{Config, LocalConfig},
    protobuf::Message as _,
    rendezvous_proto::OnGrowDeviceAttestation,
    sha2::{Digest, Sha256},
    sodiumoxide::{
        crypto::{box_, sealedbox, sign},
        randombytes,
    },
    tokio,
};
use reqwest::{Client, StatusCode};
use serde_derive::{Deserialize, Serialize};
use std::{collections::HashMap, time::Duration};
use url::Url;

const ENROLLMENT_CONTEXT: &[u8] = b"ongrow-control-device-enrollment-v1";
const HEARTBEAT_CONTEXT: &[u8] = b"ongrow-control-device-heartbeat-v1";
const UNATTENDED_KEY_CONTEXT: &[u8] = b"ongrow-control-unattended-key-v1";
const UNATTENDED_PUT_CONTEXT: &[u8] = b"ongrow-control-unattended-put-v1";
const UNATTENDED_REVOKE_CONTEXT: &[u8] = b"ongrow-control-unattended-revoke-v1";
const NONCE_BYTES: usize = 32;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const UNATTENDED_STATE_KEY: &str = "ongrow-unattended-access-state-v1";
const UNATTENDED_STATE_VERSION: u8 = 1;
const UNATTENDED_ENVELOPE_VERSION: u8 = 1;
const UNATTENDED_INGEST_FORMAT_VERSION: i64 = 1;
const UNATTENDED_INGEST_ALGORITHM: &str = "curve25519-sealed-box";
const UNATTENDED_CONSENT_VERSION: &str = "de-v1-2026-08-12";
const UNATTENDED_PASSWORD_LEN: usize = 32;
const UNATTENDED_GENERATION_LEN: usize = 16;
const UNATTENDED_OPTION_KEYS: [&str; 4] = [
    "verification-method",
    "approve-mode",
    "allow-logon-screen-password",
    "allow-only-conn-window-open",
];

#[derive(Clone, Deserialize, Serialize)]
struct UnattendedLocalState {
    version: u8,
    phase: String,
    generation: String,
    consented_at: i64,
    consent_version: String,
    previous_options: HashMap<String, Option<String>>,
    #[serde(default)]
    owned_password_fingerprint: String,
    #[serde(default)]
    revoke_retry_count: u8,
    #[serde(default)]
    revoke_retry_at: i64,
    error: String,
}

impl Default for UnattendedLocalState {
    fn default() -> Self {
        Self {
            version: UNATTENDED_STATE_VERSION,
            phase: "not_granted".to_owned(),
            generation: String::new(),
            consented_at: 0,
            consent_version: String::new(),
            previous_options: HashMap::new(),
            owned_password_fingerprint: String::new(),
            revoke_retry_count: 0,
            revoke_retry_at: 0,
            error: String::new(),
        }
    }
}

#[derive(Debug, Serialize)]
struct UnattendedActionResult {
    success: bool,
    status: &'static str,
    enabled: bool,
    retryable: bool,
    error: &'static str,
}

impl UnattendedActionResult {
    fn status(status: &'static str, enabled: bool) -> Self {
        Self {
            success: true,
            status,
            enabled,
            retryable: false,
            error: "",
        }
    }

    fn error(status: &'static str, error: &'static str, retryable: bool) -> Self {
        Self {
            success: false,
            status,
            enabled: false,
            retryable,
            error,
        }
    }
}

#[derive(Deserialize)]
struct UnattendedKeyResponse {
    key_id: String,
    algorithm: String,
    public_key: String,
    format_version: i64,
}

#[derive(Deserialize)]
struct UnattendedServerResult {
    status: String,
}

#[derive(Serialize)]
struct UnattendedKeyRequest<'a> {
    device_id: &'a str,
    request_timestamp: i64,
    request_nonce: String,
    device_signature: String,
}

#[derive(Serialize)]
struct UnattendedPutRequest<'a> {
    device_id: &'a str,
    key_id: &'a str,
    format_version: i64,
    generation: String,
    consent_version: &'static str,
    consented_at: i64,
    request_id: String,
    sealed_envelope: String,
    request_timestamp: i64,
    request_nonce: String,
    device_signature: String,
}

#[derive(Serialize)]
struct UnattendedRevokeRequest<'a> {
    device_id: &'a str,
    generation: String,
    request_id: String,
    request_timestamp: i64,
    request_nonce: String,
    device_signature: String,
}

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

#[tokio::main(flavor = "current_thread")]
pub async fn unattended_status() -> String {
    unattended_result_json(unattended_status_inner().await)
}

#[tokio::main(flavor = "current_thread")]
pub async fn enable_unattended(permissions: Permissions) -> String {
    let result = enable_unattended_inner(permissions)
        .await
        .unwrap_or_else(|error| error);
    unattended_result_json(result)
}

#[tokio::main(flavor = "current_thread")]
pub async fn revoke_unattended() -> String {
    let result = revoke_unattended_inner().await.unwrap_or_else(|error| error);
    unattended_result_json(result)
}

fn unattended_result_json(result: UnattendedActionResult) -> String {
    serde_json::to_string(&result).unwrap_or_else(|_| {
        r#"{"success":false,"status":"error","enabled":false,"retryable":true,"error":"serialization_failed"}"#
            .to_owned()
    })
}

async fn unattended_status_inner() -> UnattendedActionResult {
    if crate::get_app_name() != "OnGROW Support Desk" {
        return UnattendedActionResult::error("error", "not_ongrow_client", false);
    }
    if !configured() {
        return UnattendedActionResult::error("action_required", "control_plane_unconfigured", false);
    }

    let state = load_unattended_state();
    match state.phase.as_str() {
        "active" => {
            if !crate::ipc::is_permanent_password_set_async().await {
                return UnattendedActionResult::error(
                    "action_required",
                    "local_password_missing",
                    false,
                );
            }
            if state.owned_password_fingerprint.is_empty()
                || password_storage_fingerprint().as_deref()
                    != Some(state.owned_password_fingerprint.as_str())
            {
                return UnattendedActionResult::error(
                    "action_required",
                    "password_ownership_changed",
                    false,
                );
            }
            match crate::ipc::get_options_with_ack().await {
                Ok(options) if target_options_match(&options) => {
                    UnattendedActionResult::status("enabled", true)
                }
                Ok(_) => UnattendedActionResult::error(
                    "action_required",
                    "local_settings_changed",
                    false,
                ),
                Err(_) => UnattendedActionResult::error("action_required", "service_unavailable", true),
            }
        }
        "preparing" => recover_incomplete_activation(state)
            .await
            .unwrap_or_else(|error| error),
        "revoking" => revoke_unattended_inner()
            .await
            .unwrap_or_else(|error| error),
        "error" => UnattendedActionResult::error("error", persisted_error(&state), true),
        _ if crate::ipc::is_permanent_password_set_async().await => {
            UnattendedActionResult::error("action_required", "existing_password_conflict", false)
        }
        _ => UnattendedActionResult::status("not_granted", false),
    }
}

async fn enable_unattended_inner(
    permissions: Permissions,
) -> Result<UnattendedActionResult, UnattendedActionResult> {
    validate_unattended_prerequisites(permissions)?;

    let mut state = load_unattended_state();
    if state.phase == "active" {
        return Ok(unattended_status_inner().await);
    }
    if state.phase == "revoking" {
        return Err(UnattendedActionResult::error(
            "revoking",
            "server_revoke_pending",
            true,
        ));
    }
    if state.phase == "preparing" {
        return recover_incomplete_activation(state).await;
    }
    if state.phase == "error" && !state.generation.is_empty() {
        if rollback_local_access(&state).await.is_err() {
            return Err(record_unattended_error(
                state,
                "rollback_failed",
                "action_required",
                false,
            ));
        }
        state.phase = "revoking".to_owned();
        state.error = "server_revoke_pending".to_owned();
        state.revoke_retry_count = 0;
        state.revoke_retry_at = 0;
        save_unattended_state(&state)?;
        return revoke_unattended_inner().await;
    }

    let options = crate::ipc::get_options_with_ack()
        .await
        .map_err(|_| UnattendedActionResult::error("action_required", "service_unavailable", true))?;
    if crate::ipc::is_permanent_password_set_async().await {
        return Err(UnattendedActionResult::error(
            "action_required",
            "existing_password_conflict",
            false,
        ));
    }

    let device_id = unattended_device_id().await?;
    let base_url = control_plane_url().ok_or_else(|| {
        UnattendedActionResult::error("action_required", "control_plane_unconfigured", false)
    })?;
    let client = unattended_http_client()?;
    let key = fetch_unattended_key(&client, &base_url, &device_id).await?;

    let generation = randombytes::randombytes(UNATTENDED_GENERATION_LEN);
    let password = generate_unattended_password()?;
    let sealed = seal_unattended_envelope(&key, &generation, &password)?;
    let consented_at = hbb_common::get_time() / 1_000;

    state.phase = "preparing".to_owned();
    state.generation = crate::encode64(&generation);
    state.consented_at = consented_at;
    state.consent_version = UNATTENDED_CONSENT_VERSION.to_owned();
    state.previous_options = capture_previous_options(&options);
    state.error.clear();
    save_unattended_state(&state)?;

    let target = with_target_options(options);
    if !set_and_verify_options(target).await {
        if restore_previous_options(&state).await.is_err() {
            return Err(record_unattended_error(
                state,
                "rollback_failed",
                "action_required",
                false,
            ));
        }
        return Err(record_unattended_error(
            state,
            "settings_write_failed",
            "error",
            true,
        ));
    }

    let password_string = String::from_utf8(password).map_err(|_| {
        UnattendedActionResult::error("error", "password_generation_failed", true)
    })?;
    match crate::ipc::set_permanent_password_with_ack_async(password_string).await {
        Ok(true) => match password_storage_fingerprint() {
            Some(fingerprint) => {
                state.owned_password_fingerprint = fingerprint;
                if save_unattended_state(&state).is_err() {
                    if rollback_local_access(&state).await.is_err() {
                        return Err(record_unattended_error(
                            state,
                            "rollback_failed",
                            "action_required",
                            false,
                        ));
                    }
                    return Err(record_unattended_error(
                        state,
                        "local_state_write_failed",
                        "error",
                        true,
                    ));
                }
            }
            None => {
                let cleared = matches!(
                    crate::ipc::set_permanent_password_with_ack_async(String::new()).await,
                    Ok(true)
                ) && !crate::ipc::is_permanent_password_set_async().await;
                if !cleared || restore_previous_options(&state).await.is_err() {
                    return Err(record_unattended_error(
                        state,
                        "rollback_failed",
                        "action_required",
                        false,
                    ));
                }
                return Err(record_unattended_error(
                    state,
                    "password_ownership_unverified",
                    "action_required",
                    false,
                ));
            }
        },
        _ => {
            let _ = restore_previous_options(&state).await;
            return Err(record_unattended_error(
                state,
                "password_write_failed",
                "error",
                true,
            ));
        }
    }

    if let Err(error) = put_unattended_grant(
        &client,
        &base_url,
        &device_id,
        &key,
        &generation,
        consented_at,
        &sealed,
    )
    .await
    {
        if rollback_local_access(&state).await.is_err() {
            return Err(record_unattended_error(
                state,
                "rollback_failed",
                "action_required",
                false,
            ));
        }
        return Err(record_unattended_error(
            state,
            error.error,
            "error",
            error.retryable,
        ));
    }

    state.phase = "active".to_owned();
    save_unattended_state(&state)?;
    Ok(UnattendedActionResult::status("enabled", true))
}

async fn recover_incomplete_activation(
    mut state: UnattendedLocalState,
) -> Result<UnattendedActionResult, UnattendedActionResult> {
    if rollback_local_access(&state).await.is_err() {
        return Err(record_unattended_error(
            state,
            "rollback_failed",
            "action_required",
            false,
        ));
    }
    state.phase = "revoking".to_owned();
    state.error = "server_revoke_pending".to_owned();
    state.revoke_retry_count = 0;
    state.revoke_retry_at = 0;
    save_unattended_state(&state)?;
    revoke_unattended_inner().await
}

async fn revoke_unattended_inner(
) -> Result<UnattendedActionResult, UnattendedActionResult> {
    let mut state = load_unattended_state();
    if state.phase == "not_granted" || state.generation.is_empty() {
        return Ok(UnattendedActionResult::status("not_granted", false));
    }

    if state.phase != "revoking" {
        if rollback_local_access(&state).await.is_err() {
            return Err(record_unattended_error(
                state,
                "local_revoke_failed",
                "action_required",
                false,
            ));
        }
        state.phase = "revoking".to_owned();
        state.error = "server_revoke_pending".to_owned();
        state.revoke_retry_count = 0;
        state.revoke_retry_at = 0;
        save_unattended_state(&state)?;
    }

    let now = hbb_common::get_time() / 1_000;
    if state.revoke_retry_at > now {
        return Err(UnattendedActionResult::error(
            "revoking",
            "server_revoke_pending",
            true,
        ));
    }

    let device_id = unattended_device_id().await?;
    let generation = crate::decode64(&state.generation).map_err(|_| {
        UnattendedActionResult::error("action_required", "local_state_invalid", false)
    })?;
    if generation.len() != UNATTENDED_GENERATION_LEN {
        return Err(UnattendedActionResult::error(
            "action_required",
            "local_state_invalid",
            false,
        ));
    }
    let base_url = control_plane_url().ok_or_else(|| {
        UnattendedActionResult::error("revoking", "control_plane_unconfigured", true)
    })?;
    let client = unattended_http_client()?;
    if let Err(error) = post_unattended_revoke(
        &client,
        &base_url,
        &device_id,
        &generation,
    )
    .await
    {
        state.error = error.error.to_owned();
        state.revoke_retry_count = state.revoke_retry_count.saturating_add(1).min(8);
        let delay = (5_i64 << state.revoke_retry_count.min(6)).min(300);
        state.revoke_retry_at = now.saturating_add(delay);
        save_unattended_state(&state)?;
        return Err(UnattendedActionResult::error(
            "revoking",
            error.error,
            true,
        ));
    }

    clear_unattended_state()?;
    Ok(UnattendedActionResult::status("not_granted", false))
}

fn validate_unattended_prerequisites(
    permissions: Permissions,
) -> Result<(), UnattendedActionResult> {
    if crate::get_app_name() != "OnGROW Support Desk" {
        return Err(UnattendedActionResult::error("error", "not_ongrow_client", false));
    }
    if !configured() {
        return Err(UnattendedActionResult::error(
            "action_required",
            "control_plane_unconfigured",
            false,
        ));
    }
    if crate::ui_interface::get_connect_status().status_num != 1 {
        return Err(UnattendedActionResult::error("action_required", "offline", true));
    }
    #[cfg(target_os = "macos")]
    if !permissions.screen_recording
        || !permissions.accessibility
        || !permissions.input_monitoring
        || !permissions.network
    {
        return Err(UnattendedActionResult::error(
            "action_required",
            "permissions_incomplete",
            false,
        ));
    }
    Ok(())
}

async fn unattended_device_id() -> Result<String, UnattendedActionResult> {
    let device_id = crate::ipc::get_id_async(1_000).await;
    if is_ongrow_id(&device_id) {
        Ok(device_id)
    } else {
        Err(UnattendedActionResult::error(
            "action_required",
            "invalid_device_id",
            true,
        ))
    }
}

fn unattended_http_client() -> Result<Client, UnattendedActionResult> {
    Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .https_only(true)
        .build()
        .map_err(|_| UnattendedActionResult::error("error", "http_client_unavailable", true))
}

fn load_unattended_state() -> UnattendedLocalState {
    let raw = LocalConfig::get_option(UNATTENDED_STATE_KEY);
    if raw.is_empty() {
        return UnattendedLocalState::default();
    }
    match serde_json::from_str::<UnattendedLocalState>(&raw) {
        Ok(state) if state.version == UNATTENDED_STATE_VERSION => state,
        _ => {
            let mut state = UnattendedLocalState::default();
            state.phase = "error".to_owned();
            state.error = "local_state_invalid".to_owned();
            state
        }
    }
}

fn save_unattended_state(
    state: &UnattendedLocalState,
) -> Result<(), UnattendedActionResult> {
    let raw = serde_json::to_string(state).map_err(|_| {
        UnattendedActionResult::error("error", "local_state_write_failed", true)
    })?;
    LocalConfig::set_option(UNATTENDED_STATE_KEY.to_owned(), raw.clone());
    if LocalConfig::get_option(UNATTENDED_STATE_KEY) == raw {
        Ok(())
    } else {
        Err(UnattendedActionResult::error(
            "error",
            "local_state_write_failed",
            true,
        ))
    }
}

fn clear_unattended_state() -> Result<(), UnattendedActionResult> {
    LocalConfig::set_option(UNATTENDED_STATE_KEY.to_owned(), String::new());
    if LocalConfig::get_option(UNATTENDED_STATE_KEY).is_empty() {
        Ok(())
    } else {
        Err(UnattendedActionResult::error(
            "error",
            "local_state_write_failed",
            true,
        ))
    }
}

fn record_unattended_error(
    mut state: UnattendedLocalState,
    error: &'static str,
    status: &'static str,
    retryable: bool,
) -> UnattendedActionResult {
    state.phase = "error".to_owned();
    state.error = error.to_owned();
    let _ = save_unattended_state(&state);
    UnattendedActionResult::error(status, error, retryable)
}

fn persisted_error(state: &UnattendedLocalState) -> &'static str {
    match state.error.as_str() {
        "control_plane_unavailable" => "control_plane_unavailable",
        "control_plane_busy" => "control_plane_busy",
        "unattended_rejected" => "unattended_rejected",
        "unattended_unavailable" => "unattended_unavailable",
        "settings_write_failed" => "settings_write_failed",
        "password_write_failed" => "password_write_failed",
        "rollback_failed" => "rollback_failed",
        "local_revoke_failed" => "local_revoke_failed",
        "local_state_write_failed" => "local_state_write_failed",
        "local_state_invalid" => "local_state_invalid",
        "password_ownership_unverified" => "password_ownership_unverified",
        "password_ownership_changed" => "password_ownership_changed",
        _ => "unattended_failed",
    }
}

fn capture_previous_options(
    options: &HashMap<String, String>,
) -> HashMap<String, Option<String>> {
    UNATTENDED_OPTION_KEYS
        .iter()
        .map(|key| ((*key).to_owned(), options.get(*key).cloned()))
        .collect()
}

fn with_target_options(mut options: HashMap<String, String>) -> HashMap<String, String> {
    options.insert(
        "verification-method".to_owned(),
        "use-permanent-password".to_owned(),
    );
    options.insert("approve-mode".to_owned(), "password".to_owned());
    options.insert("allow-logon-screen-password".to_owned(), "Y".to_owned());
    options.insert("allow-only-conn-window-open".to_owned(), "N".to_owned());
    options
}

fn target_options_match(options: &HashMap<String, String>) -> bool {
    options.get("verification-method").map(String::as_str) == Some("use-permanent-password")
        && options.get("approve-mode").map(String::as_str) == Some("password")
        && options.get("allow-logon-screen-password").map(String::as_str) == Some("Y")
        && options.get("allow-only-conn-window-open").map(String::as_str) == Some("N")
}

async fn set_and_verify_options(options: HashMap<String, String>) -> bool {
    match crate::ipc::set_options_with_ack(options).await {
        Ok(true) => crate::ipc::get_options_with_ack()
            .await
            .map(|current| target_options_match(&current))
            .unwrap_or(false),
        _ => false,
    }
}

async fn restore_previous_options(state: &UnattendedLocalState) -> Result<(), ()> {
    let mut current = crate::ipc::get_options_with_ack().await.map_err(|_| ())?;
    for key in UNATTENDED_OPTION_KEYS {
        match state.previous_options.get(key).and_then(Clone::clone) {
            Some(value) => {
                current.insert(key.to_owned(), value);
            }
            None => {
                current.remove(key);
            }
        }
    }
    match crate::ipc::set_options_with_ack(current).await {
        Ok(true) => {
            let verified = crate::ipc::get_options_with_ack().await.map_err(|_| ())?;
            if UNATTENDED_OPTION_KEYS.iter().all(|key| {
                verified.get(*key).cloned()
                    == state.previous_options.get(*key).and_then(Clone::clone)
            }) {
                Ok(())
            } else {
                Err(())
            }
        }
        _ => Err(()),
    }
}

async fn rollback_local_access(state: &UnattendedLocalState) -> Result<(), ()> {
    if crate::ipc::is_permanent_password_set_async().await {
        let current = password_storage_fingerprint().ok_or(())?;
        if state.owned_password_fingerprint.is_empty()
            || current != state.owned_password_fingerprint
        {
            return Err(());
        }
        match crate::ipc::set_permanent_password_with_ack_async(String::new()).await {
            Ok(true) if !crate::ipc::is_permanent_password_set_async().await => {}
            _ => return Err(()),
        }
    }
    restore_previous_options(state).await
}

fn password_storage_fingerprint() -> Option<String> {
    let (storage, salt) = Config::get_local_permanent_password_storage_and_salt();
    password_storage_fingerprint_for(&storage, &salt)
}

fn password_storage_fingerprint_for(storage: &str, salt: &str) -> Option<String> {
    if storage.is_empty() || salt.is_empty() {
        return None;
    }
    let mut hasher = Sha256::new();
    hasher.update((storage.len() as u32).to_be_bytes());
    hasher.update(storage.as_bytes());
    hasher.update((salt.len() as u32).to_be_bytes());
    hasher.update(salt.as_bytes());
    Some(hex::encode(hasher.finalize()))
}

fn generate_unattended_password() -> Result<Vec<u8>, UnattendedActionResult> {
    hbb_common::sodiumoxide::init().map_err(|_| {
        UnattendedActionResult::error("error", "crypto_unavailable", true)
    })?;
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    const ACCEPT_BELOW: u8 = 248;
    let mut password = Vec::with_capacity(UNATTENDED_PASSWORD_LEN);
    while password.len() < UNATTENDED_PASSWORD_LEN {
        for value in randombytes::randombytes(UNATTENDED_PASSWORD_LEN) {
            if value < ACCEPT_BELOW {
                password.push(ALPHABET[(value as usize) % ALPHABET.len()]);
                if password.len() == UNATTENDED_PASSWORD_LEN {
                    break;
                }
            }
        }
    }
    Ok(password)
}

fn seal_unattended_envelope(
    key: &UnattendedKeyResponse,
    generation: &[u8],
    password: &[u8],
) -> Result<Vec<u8>, UnattendedActionResult> {
    if key.algorithm != UNATTENDED_INGEST_ALGORITHM
        || key.format_version != UNATTENDED_INGEST_FORMAT_VERSION
        || generation.len() != UNATTENDED_GENERATION_LEN
        || !(16..=128).contains(&password.len())
    {
        return Err(UnattendedActionResult::error(
            "error",
            "ingest_key_invalid",
            false,
        ));
    }
    let public = crate::decode64(&key.public_key).map_err(|_| {
        UnattendedActionResult::error("error", "ingest_key_invalid", false)
    })?;
    if public.len() != box_::PUBLICKEYBYTES {
        return Err(UnattendedActionResult::error(
            "error",
            "ingest_key_invalid",
            false,
        ));
    }
    let mut public_bytes = [0; box_::PUBLICKEYBYTES];
    public_bytes.copy_from_slice(&public);
    let mut envelope = Vec::with_capacity(19 + password.len());
    envelope.push(UNATTENDED_ENVELOPE_VERSION);
    envelope.extend_from_slice(generation);
    envelope.extend_from_slice(&(password.len() as u16).to_be_bytes());
    envelope.extend_from_slice(password);
    let sealed = sealedbox::seal(&envelope, &box_::PublicKey(public_bytes));
    envelope.fill(0);
    Ok(sealed)
}

async fn fetch_unattended_key(
    client: &Client,
    base_url: &Url,
    device_id: &str,
) -> Result<UnattendedKeyResponse, UnattendedActionResult> {
    hbb_common::sodiumoxide::init().map_err(|_| {
        UnattendedActionResult::error("error", "crypto_unavailable", true)
    })?;
    let nonce = randombytes::randombytes(NONCE_BYTES);
    let timestamp = hbb_common::get_time() / 1_000;
    let canonical = unattended_key_canonical(device_id, timestamp, &nonce);
    let signature = sign_with_device_key(&canonical).map_err(sync_to_unattended_error)?;
    let request = UnattendedKeyRequest {
        device_id,
        request_timestamp: timestamp,
        request_nonce: crate::encode64(&nonce),
        device_signature: crate::encode64(signature),
    };
    let response = client
        .post(endpoint(base_url, "v1/device/unattended-access/key").map_err(sync_to_unattended_error)?)
        .json(&request)
        .send()
        .await
        .map_err(|_| UnattendedActionResult::error("error", "control_plane_unavailable", true))?;
    if !response.status().is_success() {
        return Err(unattended_http_error(response.status()));
    }
    let key = response.json::<UnattendedKeyResponse>().await.map_err(|_| {
        UnattendedActionResult::error("error", "control_plane_response_invalid", true)
    })?;
    if key.key_id.is_empty()
        || key.key_id.len() > 64
        || key.algorithm != UNATTENDED_INGEST_ALGORITHM
        || key.format_version != UNATTENDED_INGEST_FORMAT_VERSION
    {
        return Err(UnattendedActionResult::error(
            "error",
            "ingest_key_invalid",
            false,
        ));
    }
    Ok(key)
}

async fn put_unattended_grant(
    client: &Client,
    base_url: &Url,
    device_id: &str,
    key: &UnattendedKeyResponse,
    generation: &[u8],
    consented_at: i64,
    sealed: &[u8],
) -> Result<(), UnattendedActionResult> {
    let nonce = randombytes::randombytes(NONCE_BYTES);
    let timestamp = hbb_common::get_time() / 1_000;
    let request_id = hex::encode(randombytes::randombytes(16));
    let canonical = unattended_put_canonical(
        device_id,
        &key.key_id,
        key.format_version,
        generation,
        UNATTENDED_CONSENT_VERSION,
        consented_at,
        &request_id,
        sealed,
        timestamp,
        &nonce,
    );
    let signature = sign_with_device_key(&canonical).map_err(sync_to_unattended_error)?;
    let request = UnattendedPutRequest {
        device_id,
        key_id: &key.key_id,
        format_version: key.format_version,
        generation: crate::encode64(generation),
        consent_version: UNATTENDED_CONSENT_VERSION,
        consented_at,
        request_id,
        sealed_envelope: crate::encode64(sealed),
        request_timestamp: timestamp,
        request_nonce: crate::encode64(&nonce),
        device_signature: crate::encode64(signature),
    };
    let response = client
        .put(endpoint(base_url, "v1/device/unattended-access").map_err(sync_to_unattended_error)?)
        .json(&request)
        .send()
        .await
        .map_err(|_| UnattendedActionResult::error("error", "control_plane_unavailable", true))?;
    if !response.status().is_success() {
        return Err(unattended_http_error(response.status()));
    }
    let result = response.json::<UnattendedServerResult>().await.map_err(|_| {
        UnattendedActionResult::error("error", "control_plane_response_invalid", true)
    })?;
    if result.status != "active" {
        return Err(UnattendedActionResult::error(
            "error",
            "control_plane_response_invalid",
            true,
        ));
    }
    Ok(())
}

async fn post_unattended_revoke(
    client: &Client,
    base_url: &Url,
    device_id: &str,
    generation: &[u8],
) -> Result<(), UnattendedActionResult> {
    let nonce = randombytes::randombytes(NONCE_BYTES);
    let timestamp = hbb_common::get_time() / 1_000;
    let request_id = hex::encode(randombytes::randombytes(16));
    let canonical = unattended_revoke_canonical(
        device_id,
        generation,
        &request_id,
        timestamp,
        &nonce,
    );
    let signature = sign_with_device_key(&canonical).map_err(sync_to_unattended_error)?;
    let request = UnattendedRevokeRequest {
        device_id,
        generation: crate::encode64(generation),
        request_id,
        request_timestamp: timestamp,
        request_nonce: crate::encode64(&nonce),
        device_signature: crate::encode64(signature),
    };
    let response = client
        .post(endpoint(base_url, "v1/device/unattended-access/revoke").map_err(sync_to_unattended_error)?)
        .json(&request)
        .send()
        .await
        .map_err(|_| UnattendedActionResult::error("revoking", "control_plane_unavailable", true))?;
    if response.status() == StatusCode::NOT_FOUND {
        return Ok(());
    }
    if !response.status().is_success() {
        return Err(unattended_http_error(response.status()));
    }
    let result = response.json::<UnattendedServerResult>().await.map_err(|_| {
        UnattendedActionResult::error("revoking", "control_plane_response_invalid", true)
    })?;
    if result.status != "revoked" {
        return Err(UnattendedActionResult::error(
            "revoking",
            "control_plane_response_invalid",
            true,
        ));
    }
    Ok(())
}

fn unattended_http_error(status: StatusCode) -> UnattendedActionResult {
    match status {
        StatusCode::BAD_REQUEST | StatusCode::UNAUTHORIZED | StatusCode::CONFLICT => {
            UnattendedActionResult::error("error", "unattended_rejected", false)
        }
        StatusCode::NOT_FOUND => {
            UnattendedActionResult::error("action_required", "unattended_unavailable", false)
        }
        StatusCode::TOO_MANY_REQUESTS | StatusCode::SERVICE_UNAVAILABLE => {
            UnattendedActionResult::error("error", "control_plane_busy", true)
        }
        _ => UnattendedActionResult::error("error", "control_plane_error", true),
    }
}

fn sync_to_unattended_error(error: SyncResult) -> UnattendedActionResult {
    UnattendedActionResult::error("error", error.error, error.retryable)
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

    let device_id = crate::ipc::get_id_async(1_000).await;
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

fn unattended_key_canonical(device_id: &str, timestamp: i64, nonce: &[u8]) -> Vec<u8> {
    let timestamp = timestamp.to_be_bytes();
    canonical(
        UNATTENDED_KEY_CONTEXT,
        &[device_id.as_bytes(), &timestamp, nonce],
    )
}

#[allow(clippy::too_many_arguments)]
fn unattended_put_canonical(
    device_id: &str,
    key_id: &str,
    format_version: i64,
    generation: &[u8],
    consent_version: &str,
    consented_at: i64,
    request_id: &str,
    sealed: &[u8],
    timestamp: i64,
    nonce: &[u8],
) -> Vec<u8> {
    let format_version = format_version.to_be_bytes();
    let consented_at = consented_at.to_be_bytes();
    let sealed_digest = Sha256::digest(sealed);
    let timestamp = timestamp.to_be_bytes();
    canonical(
        UNATTENDED_PUT_CONTEXT,
        &[
            device_id.as_bytes(),
            key_id.as_bytes(),
            &format_version,
            generation,
            consent_version.as_bytes(),
            &consented_at,
            request_id.as_bytes(),
            sealed_digest.as_slice(),
            &timestamp,
            nonce,
        ],
    )
}

fn unattended_revoke_canonical(
    device_id: &str,
    generation: &[u8],
    request_id: &str,
    timestamp: i64,
    nonce: &[u8],
) -> Vec<u8> {
    let timestamp = timestamp.to_be_bytes();
    canonical(
        UNATTENDED_REVOKE_CONTEXT,
        &[
            device_id.as_bytes(),
            generation,
            request_id.as_bytes(),
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

    #[test]
    fn unattended_password_has_expected_entropy_shape() {
        let password = generate_unattended_password().unwrap();
        assert_eq!(password.len(), UNATTENDED_PASSWORD_LEN);
        assert!(password.iter().all(u8::is_ascii_alphanumeric));
    }

    #[test]
    fn unattended_target_options_are_explicit() {
        let mut original = HashMap::new();
        original.insert("unrelated".to_owned(), "kept".to_owned());
        let previous = capture_previous_options(&original);
        let target = with_target_options(original);

        assert!(target_options_match(&target));
        assert_eq!(target.get("unrelated").map(String::as_str), Some("kept"));
        assert!(previous.values().all(Option::is_none));
    }

    #[test]
    fn unattended_canonical_binds_ciphertext_digest() {
        let generation = [4; UNATTENDED_GENERATION_LEN];
        let nonce = [7; NONCE_BYTES];
        let first = unattended_put_canonical(
            "OG-0001",
            "ingest-1",
            1,
            &generation,
            "de-v1",
            1_800_000_000,
            "request-1",
            b"sealed-a",
            1_800_000_001,
            &nonce,
        );
        let second = unattended_put_canonical(
            "OG-0001",
            "ingest-1",
            1,
            &generation,
            "de-v1",
            1_800_000_000,
            "request-1",
            b"sealed-b",
            1_800_000_001,
            &nonce,
        );
        assert_ne!(first, second);
        assert!(first.starts_with(UNATTENDED_PUT_CONTEXT));
    }

    #[test]
    fn password_ownership_fingerprint_binds_storage_and_salt() {
        let owned = password_storage_fingerprint_for("storage-a", "salt-a").unwrap();
        assert_eq!(
            owned,
            password_storage_fingerprint_for("storage-a", "salt-a").unwrap()
        );
        assert_ne!(
            owned,
            password_storage_fingerprint_for("storage-b", "salt-a").unwrap()
        );
        assert_ne!(
            owned,
            password_storage_fingerprint_for("storage-a", "salt-b").unwrap()
        );
        assert!(password_storage_fingerprint_for("", "salt-a").is_none());
    }
}
