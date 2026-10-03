use super::super::{verify_manifest, StreamingPayloadVerifier, SIGNATURE_DOMAIN};
use super::*;
use hbb_common::sodiumoxide::{self, crypto::sign};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

fn deadline_request_prefix_is_valid(requests: &[String]) -> bool {
    match requests {
        [] => true, // The overall deadline also covers TLS before HTTP starts.
        [manifest] => manifest == "/releases/manifest.json",
        [manifest, signature] => {
            manifest == "/releases/manifest.json" && signature == "/releases/manifest.sig"
        }
        _ => false,
    }
}

#[test]
fn deadline_request_prefix_contract() {
    let valid: &[&[&str]] = &[
        &[],
        &["/releases/manifest.json"],
        &["/releases/manifest.json", "/releases/manifest.sig"],
    ];
    let invalid: &[&[&str]] = &[
        &["/releases/9/synthetic.msi"],
        &["/foreign"],
        &["/releases/manifest.sig"],
        &["/releases/manifest.sig", "/releases/manifest.json"],
        &["/releases/manifest.json", "/releases/manifest.json"],
        &["/releases/manifest.json", "/foreign"],
        &["/releases/manifest.json", "/releases/9/synthetic.msi"],
        &[
            "/releases/manifest.json",
            "/releases/manifest.sig",
            "/releases/9/synthetic.msi",
        ],
        &[
            "/releases/manifest.json",
            "/releases/manifest.sig",
            "/releases/manifest.sig",
        ],
    ];
    for values in valid {
        let requests: Vec<String> = values.iter().map(|value| (*value).to_owned()).collect();
        assert!(deadline_request_prefix_is_valid(&requests));
    }
    for values in invalid {
        let requests: Vec<String> = values.iter().map(|value| (*value).to_owned()).collect();
        assert!(!deadline_request_prefix_is_valid(&requests));
    }
}

#[test]
fn streaming_size_hash_and_overflow_boundaries() {
    fn verifier(size: u64, bytes: &[u8]) -> StreamingPayloadVerifier {
        StreamingPayloadVerifier {
            expected_size: size,
            expected_hash: format!("{:x}", Sha256::digest(bytes)),
            received: 0,
            hash: Sha256::new(),
        }
    }
    verifier(0, b"").finalize().unwrap();
    let mut good = verifier(6, b"abcdef");
    good.update(b"").unwrap();
    good.update(b"ab").unwrap();
    good.update(b"cdef").unwrap();
    good.finalize().unwrap();
    let mut short = verifier(6, b"abcdef");
    short.update(b"abc").unwrap();
    assert_eq!(short.finalize(), Err(Error::PayloadSize));
    let mut long = verifier(2, b"ab");
    assert_eq!(long.update(b"abc"), Err(Error::PayloadSize));
    assert_eq!(long.received, 0);
    let mut tampered = verifier(2, b"ab");
    tampered.update(b"ac").unwrap();
    assert_eq!(tampered.finalize(), Err(Error::PayloadHash));
    let mut overflow = verifier(u64::MAX, b"");
    overflow.received = u64::MAX;
    assert_eq!(overflow.update(b"x"), Err(Error::PayloadSize));
    assert_eq!(overflow.received, u64::MAX);
}

#[test]
fn scheduler_initial_success_retry_parallel_drop_backward_and_ranges() {
    let start = Instant::now();
    let now = Arc::new(Mutex::new(start));
    let read = Arc::clone(&now);
    let scheduler = CheckScheduler::with_clock(Arc::new(move || *read.lock().unwrap())).unwrap();
    assert!(matches!(
        scheduler.acquire(false),
        Err(RuntimeError::NotDue)
    ));
    *now.lock().unwrap() = start + INITIAL - Duration::from_nanos(1);
    assert!(matches!(
        scheduler.acquire(false),
        Err(RuntimeError::NotDue)
    ));
    *now.lock().unwrap() = start + INITIAL;
    let lease = scheduler.acquire(false).unwrap();
    assert!(matches!(scheduler.acquire(true), Err(RuntimeError::Busy)));
    assert!(matches!(scheduler.acquire(false), Err(RuntimeError::Busy)));
    drop(lease);
    *now.lock().unwrap() = start + INITIAL + RETRY - Duration::from_nanos(1);
    assert!(matches!(
        scheduler.acquire(false),
        Err(RuntimeError::NotDue)
    ));
    *now.lock().unwrap() = start + INITIAL + RETRY;
    let mut lease = scheduler.acquire(false).unwrap();
    lease.success = true;
    drop(lease);
    *now.lock().unwrap() = start;
    assert!(matches!(
        scheduler.acquire(false),
        Err(RuntimeError::NotDue)
    ));
    *now.lock().unwrap() = start + INITIAL + RETRY + SUCCESS;
    drop(scheduler.acquire(false).unwrap());
    // A simultaneous manual caller cannot steal the active lease.
    let scheduler = Arc::new(scheduler);
    let lease = scheduler.acquire(true).unwrap();
    let other = Arc::clone(&scheduler);
    assert!(
        std::thread::spawn(move || matches!(other.acquire(true), Err(RuntimeError::Busy)))
            .join()
            .unwrap()
    );
    drop(lease);
    assert!(scheduler.acquire(true).is_ok());
    assert!(start.checked_add(Duration::MAX).is_none());
    scheduler.state.lock().unwrap().exhausted = true;
    assert!(matches!(
        scheduler.acquire(true),
        Err(RuntimeError::ClockRange)
    ));
}

#[test]
fn trusted_policy_rejects_plain_http_bad_keys_and_ambiguous_locations() {
    for (key, origin, prefix) in [
        (vec![0; 31], "https://example.test", "/releases/"),
        (vec![0; 32], "http://example.test", "/releases/"),
        (vec![0; 32], "https://example.test", "/a/../"),
    ] {
        assert!(RuntimePolicy::new(
            Product::CustomerDesk,
            Platform::WindowsX64,
            Channel::Lab,
            &key,
            7,
            origin,
            prefix
        )
        .is_err());
    }
}

#[cfg(ongrow_update_runtime_probe)]
mod native {
    use super::*;
    use std::{
        path::PathBuf,
        pin::Pin,
        task::{Context as TaskContext, Poll},
    };
    const PAYLOAD: &[u8] = b"synthetic MSI bytes, no installation";

    struct Fixture {
        root: PathBuf,
        origin: String,
        ca: Vec<u8>,
        public: sign::PublicKey,
        secret: sign::SecretKey,
    }
    impl Fixture {
        fn new() -> Self {
            sodiumoxide::init().unwrap();
            let (public, secret) = sign::gen_keypair();
            let root = PathBuf::from(
                std::env::var_os("ONGROW_RUNTIME_PROBE_ROOT").expect("native TLS probe root"),
            );
            Self {
                origin: std::env::var("ONGROW_RUNTIME_PROBE_ORIGIN")
                    .expect("native TLS probe origin"),
                ca: std::fs::read(root.join("cert.pem")).unwrap(),
                root,
                public,
                secret,
            }
        }
        fn value(&self, product: Product) -> Value {
            json!({"schema_version":1, "product": if product == Product::CustomerDesk {"customer-desk"} else {"support-console"},
                "platform":"windows-x64", "channel":"lab", "release_sequence":9, "upstream_version":"1.4.9",
                "source_sha":"0123456789abcdef0123456789abcdef01234567", "issued_at":90, "expires_at":110,
                "filename":"synthetic.msi", "size":PAYLOAD.len(), "sha256":format!("{:x}", Sha256::digest(PAYLOAD)),
                "download_url":format!("{}/releases/9/synthetic.msi", self.origin)})
        }
        fn routes(&self, value: &Value) -> Value {
            let raw = serde_json::to_vec(value).unwrap();
            let mut message = SIGNATURE_DOMAIN.to_vec();
            message.extend_from_slice(&raw);
            let sig = sign::sign_detached(&message, &self.secret);
            json!({"/releases/manifest.json":{"body":raw}, "/releases/manifest.sig":{"body":sig.as_ref()},
                "/releases/9/synthetic.msi":{"body":PAYLOAD}})
        }
        fn publish(&self, routes: &Value) {
            std::fs::write(
                self.root.join("routes.json"),
                serde_json::to_vec(routes).unwrap(),
            )
            .unwrap();
            std::fs::write(self.root.join("requests.json"), b"[]").unwrap();
        }
        fn requests(&self) -> Vec<String> {
            serde_json::from_slice(&std::fs::read(self.root.join("requests.json")).unwrap())
                .unwrap()
        }
        fn runtime(&self, product: Product, trusted_ca: bool) -> UpdateRuntime {
            let policy = RuntimePolicy::new(
                product,
                Platform::WindowsX64,
                Channel::Lab,
                self.public.as_ref(),
                7,
                &self.origin,
                "/releases/",
            )
            .unwrap();
            if !trusted_ca {
                return UpdateRuntime::new(policy).unwrap();
            }
            // Only this cfg(test) + dedicated-marker module may add the ephemeral CA.
            let client = client_builder()
                .add_root_certificate(reqwest::Certificate::from_pem(&self.ca).unwrap())
                .build()
                .unwrap();
            UpdateRuntime {
                policy,
                scheduler: CheckScheduler::new().unwrap(),
                client,
                deadline: CHECK_TIMEOUT,
            }
        }
        async fn rejected(
            &self,
            routes: Value,
            expected: RuntimeError,
            requests: usize,
            max_sink: usize,
        ) {
            self.publish(&routes);
            let runtime = self.runtime(Product::CustomerDesk, true);
            let mut sink = Vec::new();
            assert_eq!(
                runtime
                    .check(&mut sink, 100, LastAcceptedSequence::Known(8), true)
                    .await
                    .err(),
                Some(expected)
            );
            assert_eq!(self.requests().len(), requests);
            assert!(sink.len() <= max_sink);
        }
    }

    #[derive(Default)]
    struct Sink {
        bytes: Vec<u8>,
        flushed: bool,
        fail_write: bool,
        fail_flush: bool,
    }
    impl AsyncWrite for Sink {
        fn poll_write(
            mut self: Pin<&mut Self>,
            _: &mut TaskContext<'_>,
            bytes: &[u8],
        ) -> Poll<std::io::Result<usize>> {
            if self.fail_write {
                return Poll::Ready(Err(std::io::ErrorKind::Other.into()));
            }
            self.bytes.extend_from_slice(bytes);
            Poll::Ready(Ok(bytes.len()))
        }
        fn poll_flush(
            mut self: Pin<&mut Self>,
            _: &mut TaskContext<'_>,
        ) -> Poll<std::io::Result<()>> {
            if self.fail_flush {
                return Poll::Ready(Err(std::io::ErrorKind::Other.into()));
            }
            self.flushed = true;
            Poll::Ready(Ok(()))
        }
        fn poll_shutdown(
            self: Pin<&mut Self>,
            _: &mut TaskContext<'_>,
        ) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn original_transport_real_tls_success_and_rejections() {
        let f = Fixture::new();
        for product in [Product::CustomerDesk, Product::SupportConsole] {
            for chunked in [false, true] {
                let mut routes = f.routes(&f.value(product));
                if chunked {
                    for route in routes.as_object_mut().unwrap().values_mut() {
                        route["chunked"] = json!(true);
                    }
                }
                let published_raw: Vec<u8> =
                    serde_json::from_value(routes["/releases/manifest.json"]["body"].clone())
                        .unwrap();
                let published_signature: Vec<u8> =
                    serde_json::from_value(routes["/releases/manifest.sig"]["body"].clone())
                        .unwrap();
                f.publish(&routes);
                let runtime = f.runtime(product, true);
                let mut sink = Sink::default();
                let verified = match runtime
                    .check(&mut sink, 100, LastAcceptedSequence::Known(8), true)
                    .await
                    .unwrap()
                {
                    CheckOutcome::Downloaded(verified) => verified,
                    CheckOutcome::NoUpdate => panic!("new release must download"),
                };
                assert_eq!(verified.candidate().manifest().release_sequence, 9);
                let (raw, signature) = verified.signed_manifest();
                assert_eq!(raw, published_raw);
                assert_eq!(signature.as_slice(), published_signature);
                assert!(raw.len() <= MAX_MANIFEST_BYTES);
                let reverified = verify_manifest(
                    raw,
                    signature,
                    f.public.as_ref(),
                    &runtime.policy.context(100, LastAcceptedSequence::Known(8)),
                )
                .unwrap();
                reverified.verify_payload(&sink.bytes).unwrap();
                assert_eq!(sink.bytes, PAYLOAD);
                assert!(sink.flushed);
                assert_eq!(f.requests().len(), 3);
                assert!(matches!(
                    runtime
                        .check(&mut sink, 100, LastAcceptedSequence::Known(8), false)
                        .await,
                    Err(RuntimeError::NotDue)
                ));
                assert_eq!(f.requests().len(), 3);
            }
        }
        let good = || f.routes(&f.value(Product::CustomerDesk));
        // Authenticated unchanged/older releases are a successful check, not retry.
        // The protected sequence is supplied afresh for every check on one runtime.
        let runtime = f.runtime(Product::CustomerDesk, true);
        f.publish(&good());
        let mut initial = Vec::new();
        assert!(matches!(
            runtime
                .check(&mut initial, 100, LastAcceptedSequence::Known(8), true)
                .await,
            Ok(CheckOutcome::Downloaded(_))
        ));
        for sequence in [9, 8, 7] {
            let mut manifest = f.value(Product::CustomerDesk);
            manifest["release_sequence"] = json!(sequence);
            f.publish(&f.routes(&manifest));
            let mut sink = Sink::default();
            assert!(matches!(
                runtime
                    .check(&mut sink, 100, LastAcceptedSequence::Known(9), true)
                    .await,
                Ok(CheckOutcome::NoUpdate)
            ));
            assert_eq!(f.requests().len(), 2);
            assert!(sink.bytes.is_empty());
            assert!(!sink.flushed);
            assert!(
                runtime
                    .scheduler
                    .state
                    .lock()
                    .unwrap()
                    .next
                    .duration_since(Instant::now())
                    > SUCCESS - Duration::from_secs(1)
            );
        }
        f.publish(&good());
        let mut sink = Sink::default();
        assert!(matches!(
            runtime
                .check(&mut sink, 100, LastAcceptedSequence::Unknown, true)
                .await,
            Err(RuntimeError::Verification(Error::UnknownReleaseState))
        ));
        assert!(f.requests().is_empty());
        assert!(sink.bytes.is_empty());
        for (field, value, error) in [
            ("expires_at", json!(100), Error::InvalidTime),
            ("issued_at", json!(101), Error::InvalidTime),
            ("channel", json!("stable"), Error::WrongContext),
            ("product", json!("support-console"), Error::WrongContext),
        ] {
            let mut value_manifest = f.value(Product::CustomerDesk);
            value_manifest[field] = value;
            f.rejected(f.routes(&value_manifest), error.into(), 2, 0)
                .await;
        }
        let mut large = f.value(Product::CustomerDesk);
        large["size"] = json!(MAX_PAYLOAD_BYTES + 1);
        f.rejected(f.routes(&large), RuntimeError::PayloadLimit, 2, 0)
            .await;
        f.publish(&good());
        let runtime = f.runtime(Product::CustomerDesk, true);
        let mut sink = Sink::default();
        assert!(matches!(
            runtime
                .check(&mut sink, 100, LastAcceptedSequence::Unknown, true)
                .await,
            Err(RuntimeError::Verification(Error::UnknownReleaseState))
        ));
        assert!(f.requests().is_empty());
        assert!(sink.bytes.is_empty());
        assert!(!sink.flushed);
        f.publish(&good());
        let mut runtime = f.runtime(Product::CustomerDesk, true);
        runtime.policy.public_key = sign::gen_keypair().0 .0;
        assert!(matches!(
            runtime
                .check(&mut sink, 100, LastAcceptedSequence::Known(8), true)
                .await,
            Err(RuntimeError::Verification(Error::InvalidSignature))
        ));
        assert_eq!(f.requests().len(), 2);
        assert!(sink.bytes.is_empty());
        let mut routes = good();
        routes["/releases/manifest.json"]["body"]
            .as_array_mut()
            .unwrap()
            .push(json!(32));
        f.rejected(routes, Error::InvalidSignature.into(), 2, 0)
            .await;
        let mut routes = good();
        routes["/releases/9/synthetic.msi"]["body"][0] = json!(42);
        f.rejected(routes, Error::PayloadHash.into(), 3, PAYLOAD.len())
            .await;
        for size in [PAYLOAD.len() - 1, PAYLOAD.len() + 1] {
            let mut routes = good();
            routes["/releases/9/synthetic.msi"]["body"] = json!(vec![1; size]);
            routes["/releases/9/synthetic.msi"]["chunked"] = json!(true);
            f.rejected(routes, Error::PayloadSize.into(), 3, PAYLOAD.len())
                .await;
        }
        let mut routes = good();
        routes["/releases/9/synthetic.msi"]["length"] = json!(PAYLOAD.len() + 1);
        f.rejected(routes, RuntimeError::ContentLength, 3, 0).await;
        let mut routes = good();
        routes["/releases/manifest.json"]["chunked"] = json!(true);
        routes["/releases/manifest.json"]["length"] = json!(3);
        f.rejected(routes, RuntimeError::ContentLength, 1, 0).await;
        let mut routes = good();
        routes["/releases/manifest.json"]["duplicate_length"] = json!(routes
            ["/releases/manifest.json"]["body"]
            .as_array()
            .unwrap()
            .len());
        f.rejected(routes, RuntimeError::ContentLength, 1, 0).await;
        let mut routes = good();
        routes["/releases/manifest.json"]["duplicate_length"] = json!(1);
        f.rejected(routes, RuntimeError::Transport, 1, 0).await;
        for path in [
            "/releases/manifest.json",
            "/releases/manifest.sig",
            "/releases/9/synthetic.msi",
        ] {
            let count = if path.ends_with("json") {
                1
            } else if path.ends_with("sig") {
                2
            } else {
                3
            };
            let mut routes = good();
            routes[path]["encoding"] = json!("gzip");
            f.rejected(routes, RuntimeError::Encoding, count, 0).await;
            let mut routes = good();
            routes[path]["status"] = json!(302);
            routes[path]["location"] = json!(format!("{}/redirect-target", f.origin));
            f.rejected(routes, RuntimeError::Status, count, 0).await;
            assert!(!f.requests().iter().any(|s| s == "/redirect-target"));
        }
        let mut routes = good();
        routes["/releases/9/synthetic.msi"]["status"] = json!(206);
        f.rejected(routes, RuntimeError::Status, 3, 0).await;
        for path in ["/releases/manifest.json", "/releases/manifest.sig"] {
            let count = if path.ends_with("json") { 1 } else { 2 };
            let cap = if path.ends_with("json") {
                MAX_MANIFEST_BYTES
            } else {
                64
            };
            let mut routes = good();
            routes[path]["body"] = json!(vec![0; cap + 1]);
            f.rejected(routes.clone(), RuntimeError::ContentLength, count, 0)
                .await;
            routes[path]["chunked"] = json!(true);
            f.rejected(routes, RuntimeError::MetadataSize, count, 0)
                .await;
        }
        let mut routes = good();
        routes["/releases/manifest.sig"]["body"] = json!(vec![0; 63]);
        routes["/releases/manifest.sig"]["chunked"] = json!(true);
        f.rejected(routes, RuntimeError::MetadataSize, 2, 0).await;
        let mut routes = good();
        routes["/releases/9/synthetic.msi"]["reset"] = json!(true);
        f.rejected(routes, RuntimeError::Transport, 3, PAYLOAD.len())
            .await;
        f.publish(&good());
        let runtime = f.runtime(Product::CustomerDesk, false);
        let mut sink = Vec::new();
        assert!(matches!(
            runtime
                .check(&mut sink, 100, LastAcceptedSequence::Known(8), true)
                .await,
            Err(RuntimeError::Transport)
        ));
        assert!(f.requests().is_empty());
        assert!(sink.is_empty());
        let mut routes = good();
        for route in routes.as_object_mut().unwrap().values_mut() {
            route["delay"] = json!(0.12);
        }
        f.publish(&routes);
        let mut runtime = f.runtime(Product::CustomerDesk, true);
        runtime.deadline = Duration::from_millis(200);
        assert!(matches!(
            runtime
                .check(&mut sink, 100, LastAcceptedSequence::Known(8), true)
                .await,
            Err(RuntimeError::Deadline)
        ));
        let deadline_requests = f.requests();
        assert!(
            deadline_request_prefix_is_valid(&deadline_requests),
            "deadline must stop at an exact metadata-only request prefix, possibly before HTTP"
        );
        assert!(sink.is_empty());
        // Wait for the bounded server handler to finish before changing routes.
        tokio::time::sleep(Duration::from_millis(150)).await;
        for fail_flush in [false, true] {
            f.publish(&good());
            let runtime = f.runtime(Product::CustomerDesk, true);
            let mut sink = Sink {
                fail_write: !fail_flush,
                fail_flush,
                ..Sink::default()
            };
            assert!(matches!(
                runtime
                    .check(&mut sink, 100, LastAcceptedSequence::Known(8), true)
                    .await,
                Err(RuntimeError::Sink)
            ));
            assert!(!sink.flushed);
            assert_eq!(f.requests().len(), 3);
        }
        // Actual in-flight cancellation releases single-flight without a success interval.
        let mut routes = good();
        routes["/releases/manifest.json"]["delay"] = json!(0.15);
        f.publish(&routes);
        let runtime = f.runtime(Product::CustomerDesk, true);
        let mut sink = Vec::new();
        {
            let check = runtime.check(&mut sink, 100, LastAcceptedSequence::Known(8), true);
            tokio::pin!(check);
            tokio::select! { _ = &mut check => panic!("must still be pending"),
            _ = tokio::time::sleep(Duration::from_millis(40)) => {} }
            let mut other = Vec::new();
            assert!(matches!(
                runtime
                    .check(&mut other, 100, LastAcceptedSequence::Known(8), true)
                    .await,
                Err(RuntimeError::Busy)
            ));
        }
        assert!(!runtime.scheduler.state.lock().unwrap().active);
        assert!(matches!(
            runtime
                .check(&mut sink, 100, LastAcceptedSequence::Known(8), false)
                .await,
            Err(RuntimeError::NotDue)
        ));
        tokio::time::sleep(Duration::from_millis(200)).await;
        f.publish(&good());
        runtime
            .check(&mut sink, 100, LastAcceptedSequence::Known(8), true)
            .await
            .unwrap();
        assert_eq!(f.requests().len(), 3);
        println!("NATIVE_TLS_ORIGINAL_TRANSPORT_PASS");
    }
}
