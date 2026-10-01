use super::*;
use serde_json::{json, Value};

const PAYLOAD: &[u8] = b"synthetic complete Setup.exe bytes";

struct Fixture {
    public: sign::PublicKey,
    secret: sign::SecretKey,
    location: DownloadLocation,
}

impl Fixture {
    fn new() -> Self {
        sodiumoxide::init().unwrap();
        let (public, secret) = sign::gen_keypair();
        Self {
            public,
            secret,
            location: DownloadLocation::new("https://updates.example.test", "/releases/").unwrap(),
        }
    }

    fn context(&self) -> Context<'_> {
        Context {
            product: Product::CustomerDesk,
            platform: Platform::WindowsX64,
            channel: Channel::Lab,
            now_unix_seconds: 100,
            baked_current_sequence: 7,
            last_accepted_sequence: LastAcceptedSequence::Known(8),
            download_location: &self.location,
        }
    }

    fn signature(&self, raw: &[u8]) -> Vec<u8> {
        let mut message = SIGNATURE_DOMAIN.to_vec();
        message.extend_from_slice(raw);
        sign::sign_detached(&message, &self.secret)
            .as_ref()
            .to_vec()
    }

    fn verify(&self, value: Value) -> Result<VerifiedCandidate, Error> {
        let raw = serde_json::to_vec(&value).unwrap();
        verify_manifest(
            &raw,
            &self.signature(&raw),
            self.public.as_ref(),
            &self.context(),
        )
    }
}

fn manifest() -> Value {
    json!({
        "schema_version": 1,
        "product": "customer-desk",
        "platform": "windows-x64",
        "channel": "lab",
        "release_sequence": 9,
        "upstream_version": "1.4.9",
        "source_sha": "0123456789abcdef0123456789abcdef01234567",
        "issued_at": 90,
        "expires_at": 110,
        "filename": "ongrow-desk-setup.exe",
        "size": PAYLOAD.len(),
        "sha256": format!("{:x}", Sha256::digest(PAYLOAD)),
        "download_url": "https://updates.example.test/releases/9/ongrow-desk-setup.exe"
    })
}

#[test]
fn valid_signature_and_complete_payload_for_both_products() {
    let f = Fixture::new();
    let candidate = f.verify(manifest()).unwrap();
    candidate.verify_payload(PAYLOAD).unwrap();
    assert_eq!(candidate.manifest().release_sequence, 9);
    let mut value = manifest();
    value["product"] = json!("support-console");
    let raw = serde_json::to_vec(&value).unwrap();
    let mut context = f.context();
    context.product = Product::SupportConsole;
    verify_manifest(&raw, &f.signature(&raw), f.public.as_ref(), &context).unwrap();
}

#[test]
fn tampering_foreign_key_and_domain_mismatch_fail_before_parsing() {
    let f = Fixture::new();
    let raw = serde_json::to_vec(&manifest()).unwrap();
    let signature = f.signature(&raw);
    let mut changed = raw.clone();
    changed.push(b' '); // Semantically unchanged JSON still invalidates signature.
    assert_eq!(
        verify_manifest(&changed, &signature, f.public.as_ref(), &f.context()).unwrap_err(),
        Error::InvalidSignature
    );
    let foreign = Fixture::new();
    assert_eq!(
        verify_manifest(&raw, &signature, foreign.public.as_ref(), &f.context()).unwrap_err(),
        Error::InvalidSignature
    );
    let no_domain = sign::sign_detached(&raw, &f.secret);
    assert_eq!(
        verify_manifest(&raw, no_domain.as_ref(), f.public.as_ref(), &f.context()).unwrap_err(),
        Error::InvalidSignature
    );
    assert_eq!(
        verify_manifest(b"not JSON", &signature, f.public.as_ref(), &f.context()).unwrap_err(),
        Error::InvalidSignature
    );
}

#[test]
fn exact_key_signature_and_manifest_size_limits() {
    let f = Fixture::new();
    let raw = serde_json::to_vec(&manifest()).unwrap();
    let signature = f.signature(&raw);
    for length in [0, 31, 33, 64] {
        assert_eq!(
            verify_manifest(&raw, &signature, &vec![0; length], &f.context()).unwrap_err(),
            Error::InvalidKey
        );
    }
    for length in [0, 1, 63, 65, 96] {
        assert_eq!(
            verify_manifest(&raw, &vec![0; length], f.public.as_ref(), &f.context()).unwrap_err(),
            Error::InvalidSignature
        );
    }
    let oversized = vec![b' '; MAX_MANIFEST_BYTES + 1];
    assert_eq!(
        verify_manifest(&oversized, &[], f.public.as_ref(), &f.context()).unwrap_err(),
        Error::ManifestTooLarge
    );
    let mut at_limit = raw.clone();
    at_limit.resize(MAX_MANIFEST_BYTES, b' ');
    verify_manifest(
        &at_limit,
        &f.signature(&at_limit),
        f.public.as_ref(),
        &f.context(),
    )
    .unwrap();
}

#[test]
fn signed_malformed_unknown_missing_and_duplicate_fields_rejected() {
    let f = Fixture::new();
    let raw = serde_json::to_string(&manifest()).unwrap();
    let duplicate = format!("{{\"schema_version\":1,{}", &raw[1..]);
    let escaped_duplicate = format!("{{\"schema_\\u0076ersion\":1,{}", &raw[1..]);
    for raw in [
        "not JSON".to_owned(),
        duplicate,
        escaped_duplicate,
        format!("{} true", raw),
    ] {
        assert_eq!(
            verify_manifest(
                raw.as_bytes(),
                &f.signature(raw.as_bytes()),
                f.public.as_ref(),
                &f.context()
            )
            .unwrap_err(),
            Error::InvalidManifest
        );
    }
    for (field, value) in [
        ("unknown", json!(1)),
        ("schema_version", json!(2)),
        ("platform", json!("windows-arm64")),
        ("product", json!("other")),
        ("channel", json!("nightly")),
        ("release_sequence", json!(-1)),
        ("issued_at", json!("90")),
        ("size", json!(0)),
        ("source_sha", json!("bad")),
        ("sha256", json!("F".repeat(64))),
        ("upstream_version", json!("")),
    ] {
        let mut value_manifest = manifest();
        value_manifest[field] = value;
        assert_eq!(
            f.verify(value_manifest).unwrap_err(),
            Error::InvalidManifest,
            "{field}"
        );
    }
    for field in manifest().as_object().unwrap().keys() {
        let mut value = manifest();
        value.as_object_mut().unwrap().remove(field);
        assert_eq!(
            f.verify(value).unwrap_err(),
            Error::InvalidManifest,
            "{field}"
        );
    }
}

#[test]
fn exact_context_time_replay_and_unknown_state_fail_closed() {
    let f = Fixture::new();
    for (field, value, error) in [
        ("product", json!("support-console"), Error::WrongContext),
        ("channel", json!("stable"), Error::WrongContext),
        ("issued_at", json!(101), Error::InvalidTime),
        ("expires_at", json!(100), Error::InvalidTime),
        ("expires_at", json!(89), Error::InvalidTime),
        ("release_sequence", json!(8), Error::NonIncreasingSequence),
        ("release_sequence", json!(7), Error::NonIncreasingSequence),
        ("release_sequence", json!(0), Error::NonIncreasingSequence),
    ] {
        let mut value_manifest = manifest();
        value_manifest[field] = value;
        assert_eq!(f.verify(value_manifest).unwrap_err(), error, "{field}");
    }
    let raw = serde_json::to_vec(&manifest()).unwrap();
    let signature = f.signature(&raw);
    let mut context = f.context();
    context.last_accepted_sequence = LastAcceptedSequence::Unknown;
    assert_eq!(
        verify_manifest(&raw, &signature, f.public.as_ref(), &context).unwrap_err(),
        Error::UnknownReleaseState
    );
    context.last_accepted_sequence = LastAcceptedSequence::Known(0);
    context.baked_current_sequence = 9;
    assert_eq!(
        verify_manifest(&raw, &signature, f.public.as_ref(), &context).unwrap_err(),
        Error::NonIncreasingSequence
    );
}

#[test]
fn complete_bytes_size_and_hash_are_required() {
    let f = Fixture::new();
    let candidate = f.verify(manifest()).unwrap();
    assert_eq!(
        candidate.verify_payload(&PAYLOAD[..PAYLOAD.len() - 1]),
        Err(Error::PayloadSize)
    );
    let mut changed = PAYLOAD.to_vec();
    changed[0] ^= 1;
    assert_eq!(candidate.verify_payload(&changed), Err(Error::PayloadHash));
    let mut wrong_size = manifest();
    wrong_size["size"] = json!(PAYLOAD.len() + 1);
    assert_eq!(
        f.verify(wrong_size).unwrap().verify_payload(PAYLOAD),
        Err(Error::PayloadSize)
    );
    let mut wrong_hash = manifest();
    wrong_hash["sha256"] = json!("0".repeat(64));
    assert_eq!(
        f.verify(wrong_hash).unwrap().verify_payload(PAYLOAD),
        Err(Error::PayloadHash)
    );
}

#[test]
fn unsafe_filename_and_windows_aliases_rejected() {
    let f = Fixture::new();
    for filename in [
        "",
        ".",
        "..",
        "../setup.exe",
        "dir\\setup.exe",
        "setup.exe:ads",
        "C:setup.exe",
        "CON",
        "con.exe",
        "AUX.txt",
        "NUL.exe",
        "PRN.exe",
        "COM1.exe",
        "LPT9.exe",
        "setup.exe.",
        "setup.exe ",
        "COM¹.exe",
        "setup%2eexe",
    ] {
        let mut value = manifest();
        value["filename"] = json!(filename);
        assert_eq!(
            f.verify(value).unwrap_err(),
            Error::InvalidFilename,
            "{filename}"
        );
    }
}

#[test]
fn strict_https_origin_and_path_allowlist_reject_normalization_bypasses() {
    let f = Fixture::new();
    for url in [
        "http://updates.example.test/releases/9/ongrow-desk-setup.exe",
        "https://evil.example.test/releases/9/ongrow-desk-setup.exe",
        "https://updates.example.test.evil.test/releases/9/ongrow-desk-setup.exe",
        "https://user@updates.example.test/releases/9/ongrow-desk-setup.exe",
        "https://user:pass@updates.example.test/releases/9/ongrow-desk-setup.exe",
        "https://updates.example.test:443/releases/9/ongrow-desk-setup.exe",
        "https://updates.example.test/releases/9/ongrow-desk-setup.exe?download=1",
        "https://updates.example.test/releases/9/ongrow-desk-setup.exe#fragment",
        "https://updates.example.test/releases-lookalike/9/ongrow-desk-setup.exe",
        "https://updates.example.test/releases/../outside/ongrow-desk-setup.exe",
        "https://updates.example.test/releases/%2e%2e/ongrow-desk-setup.exe",
        "https://updates.example.test/releases/9%2fother/ongrow-desk-setup.exe",
        "https://updates.example.test/releases/9%5cother/ongrow-desk-setup.exe",
        "https://updates.example.test/releases/9/other.exe",
        "https://updates.example.test/releases//ongrow-desk-setup.exe",
        "https://updates.example.test/releases\\9/ongrow-desk-setup.exe",
        "https://updates.example.test/releases/9/ongrow-desk-setup.exe:ads",
        "https://updates.example.test/releases/9/ongrow-desk-setup.exe\n",
    ] {
        let mut value = manifest();
        value["download_url"] = json!(url);
        assert_eq!(
            f.verify(value).unwrap_err(),
            Error::InvalidDownloadLocation,
            "{url}"
        );
    }
    for (origin, prefix) in [
        ("http://updates.example.test", "/releases/"),
        ("https://user@updates.example.test", "/releases/"),
        ("https://updates.example.test:443", "/releases/"),
        ("https://updates.example.test", "/releases/../"),
        ("https://updates.example.test", "/releases%2f/"),
        ("https://updates.example.test", "/releases"),
        ("https://updates.example.test", "/"),
    ] {
        assert!(DownloadLocation::new(origin, prefix).is_err());
    }
}

#[test]
fn product_boundary_stops_network_and_apply_callbacks() {
    for name in ["OnGROW Support Desk", "OnGROW Support Console"] {
        let mut network_or_apply_calls = 0;
        let mut enter = || -> Result<(), Error> {
            upstream_allowed(name, None)?;
            network_or_apply_calls += 1;
            Ok(())
        };
        assert_eq!(enter(), Err(Error::UpstreamDisabled));
        assert_eq!(network_or_apply_calls, 0);
    }
    assert_eq!(upstream_allowed("RustDesk", None), Ok(()));
    for role in ["customer-desk", "support-console"] {
        assert_eq!(
            upstream_allowed("Renamed", Some(role)),
            Err(Error::UpstreamDisabled)
        );
    }
}

#[test]
fn baked_product_latch_cannot_be_changed_by_later_branding() {
    for name in ["OnGROW Support Desk", "OnGROW Support Console"] {
        let policy = ProductPolicy::default();
        assert_eq!(
            policy.require_upstream_allowed(),
            Err(Error::UpstreamDisabled)
        );
        policy.initialize(name, None);
        policy.initialize("RustDesk", None);
        assert_eq!(
            policy.require_upstream_allowed(),
            Err(Error::UpstreamDisabled)
        );
    }
    let plain = ProductPolicy::default();
    plain.initialize("RustDesk", None);
    assert_eq!(plain.require_upstream_allowed(), Ok(()));
    let compiled = ProductPolicy::default();
    compiled.initialize("Renamed", Some("support-console"));
    compiled.initialize("RustDesk", None);
    assert_eq!(
        compiled.require_upstream_allowed(),
        Err(Error::UpstreamDisabled)
    );
}

#[test]
fn early_cli_guard_only_matches_actual_update_command() {
    assert!(is_update_command(["--update"]));
    assert!(is_update_command([
        "--elevate",
        "--run-as-system",
        "--update"
    ]));
    assert!(is_update_command([
        "--quick_support",
        "--no-server",
        "--update",
        "file.dmg"
    ]));
    assert!(!is_update_command(["--install"]));
    assert!(!is_update_command(["--silent-install", "--update"]));
    assert!(!is_update_command(["--connect", "--update"]));
    assert!(!is_update_command(Vec::<String>::new()));
}

#[test]
fn each_upstream_entry_has_a_first_statement_guard() {
    // Source coverage, not a platform-lifecycle test. Run native tests in CI.
    for (source, functions) in [
        (
            include_str!("../common.rs"),
            vec!["check_software_update", "do_check_software_update"],
        ),
        (
            include_str!("../updater.rs"),
            vec![
                "start_auto_update",
                "manually_check_update",
                "stop_auto_update",
                "start_auto_update_check",
                "start_auto_update_check_",
                "check_update",
                "update_new_version",
                "get_download_file_from_url",
                "start_auto_update_macos",
                "check_update_as_root",
            ],
        ),
        (include_str!("../ui_interface.rs"), vec!["update_me"]),
        (
            include_str!("../platform/windows.rs"),
            vec![
                "prepare_custom_client_update",
                "update_me",
                "update_to",
                "update_me_msi",
                "handle_custom_client_staging_dir_before_update",
                "try_remove_temp_update_files",
            ],
        ),
        (
            include_str!("../platform/macos.rs"),
            vec![
                "update_me",
                "update_to",
                "update_from_dmg",
                "update_from_dmg_as_root",
                "extract_update_dmg",
                "update_extracted",
                "try_remove_temp_update_dir",
            ],
        ),
    ] {
        for name in functions {
            let marker = format!("fn {name}(");
            let body = source
                .split_once(&marker)
                .unwrap()
                .1
                .split_once('{')
                .unwrap()
                .1
                .trim_start();
            assert!(
                body.starts_with("crate::ongrow_update::require_upstream_allowed()")
                    || body.starts_with("if crate::ongrow_update::require_upstream_allowed()")
                    || body.starts_with(
                        "if let Err(err) = crate::ongrow_update::require_upstream_allowed()"
                    ),
                "unguarded {name}"
            );
        }
    }
    let main = include_str!("../core_main.rs");
    for body in main.split("if args[0] == \"--update\" {").skip(1) {
        assert!(body
            .trim_start()
            .starts_with("if let Err(err) = crate::ongrow_update::require_upstream_allowed()"));
    }
    assert_eq!(main.matches("if args[0] == \"--update\" {").count(), 2);
    let main_body = main.split_once("pub fn core_main()").unwrap().1;
    assert!(
        main_body.find("initialize_product_policy()").unwrap()
            < main_body.find("global_init()").unwrap()
    );
    let early_guard = main_body.find("is_update_command(").unwrap();
    assert!(early_guard < main_body.find("global_init()").unwrap());
    assert!(
        main_body[early_guard..]
            .find("require_upstream_allowed()")
            .unwrap()
            < main_body[early_guard..].find("global_init()").unwrap()
    );
    for (source, marker) in [
        (include_str!("../ui_interface.rs"), "pub fn goto_install()"),
        (include_str!("../platform/windows.rs"), "pub fn install_me("),
    ] {
        let body = source.split_once(marker).unwrap().1;
        let body = body
            .split_once('{')
            .unwrap()
            .1
            .split("\npub fn ")
            .next()
            .unwrap();
        assert!(!body.contains("require_upstream_allowed"));
    }
    let custom = include_str!("../common.rs")
        .split_once("pub fn read_custom_client(")
        .unwrap()
        .1;
    assert!(
        custom.find("initialize_product_policy()").unwrap()
            < custom.find("APP_NAME.write()").unwrap()
    );
}
