//! Build-provenance check for `geekcli update`: the archive about to be
//! installed must carry a GitHub artifact attestation, signed through the
//! public Sigstore instance by this repository's release workflow on `main`.
//!
//! The `.sha256` beside an archive and GitHub's per-asset digest both come
//! from the same release, so whoever can change the release can change them
//! too. The attestation cannot be forged that way: its certificate is issued
//! by Sigstore's Fulcio to the workflow run's OIDC identity and the signature
//! is logged in Rekor, and both are checked against Sigstore's trust root
//! compiled into this binary. Checking it is what `gh attestation verify
//! <archive> --repo RealGeeks/geekcli` does, without needing `gh`.
//!
//! The expected signer is fixed here, not taken from `GEEKCLI_REPO`, so
//! pointing the updater somewhere else cannot also change who it trusts.
//!
//! The trust root is the one `sigstore-trust-root` embeds; it is not
//! refreshed over TUF (that would pull in a second HTTP stack). When Sigstore
//! adds a log or CA that a future release is signed through, older binaries
//! will refuse that release until updated another way (the install script,
//! or `GEEKCLI_SKIP_PROVENANCE=1` once): a failure to the safe side. Keep
//! the sigstore crates current so each release carries a fresh root.
//!
//! The sigstore crates' default crypto backend is aws-lc, which would add
//! about 3 MB and a second C build; [`ring_backend`] supplies the few
//! verification primitives they need from ring, already linked for TLS.

use std::fmt::Write as _;
use std::time::Duration;

use base64::Engine as _;
use serde_json::Value;
use sha2::{Digest, Sha256};
use sigstore_trust_root::{SigstoreInstance, TrustedRoot};
use sigstore_types::{Bundle, Sha256Hash};
use sigstore_verify::{IdentityMatcher, VerificationPolicy, Verifier};

/// The repository whose attestations are looked up and trusted.
pub const REPO: &str = "RealGeeks/geekcli";
/// The certificate identity of a release build: `release.yml` run from `main`
/// (release-please's push, or a manual rebuild dispatched from `main`).
pub const SIGNER: &str =
    "https://github.com/RealGeeks/geekcli/.github/workflows/release.yml@refs/heads/main";
/// GitHub Actions' OIDC issuer.
pub const ISSUER: &str = "https://token.actions.githubusercontent.com";
/// What `actions/attest-build-provenance` attests.
pub const PREDICATE: &str = "https://slsa.dev/provenance/v1";
/// Set to skip the check (air-gapped mirrors); only the sha256 is checked then.
pub const SKIP_ENV: &str = "GEEKCLI_SKIP_PROVENANCE";

/// Hex SHA-256 of a file, streamed.
pub fn sha256_file(path: &std::path::Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0; 64 * 1024];
    loop {
        let n = std::io::Read::read(&mut file, &mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .fold(String::with_capacity(64), |mut hex, b| {
            let _ = write!(hex, "{b:02x}");
            hex
        }))
}

/// Fetch the attestations GitHub holds for `digest` (hex SHA-256) and check
/// that one of them is a valid provenance attestation from [`SIGNER`].
/// `api_base` is `https://api.github.com` unless tests point it elsewhere.
pub fn check(api_base: &str, token: Option<&str>, digest: &str) -> Result<(), String> {
    let attestations = fetch(api_base, token, digest)?;
    verify(&attestations, digest)
}

fn fetch(api_base: &str, token: Option<&str>, digest: &str) -> Result<Value, String> {
    use self_update::errors::Error as HttpError;
    use self_update::http_client::{header, HeaderMap, HttpClient, UreqClient};

    let url = format!(
        "{}/repos/{REPO}/attestations/sha256:{digest}",
        api_base.trim_end_matches('/')
    );
    // The client the download itself used (platform trust store, proxy from
    // the environment), so a lookup works wherever the download did. Only
    // these headers are sent: the site's API key never goes near GitHub.
    let mut headers = HeaderMap::new();
    let mut add = |name: header::HeaderName, value: &str| {
        if let Ok(value) = header::HeaderValue::from_str(value) {
            headers.insert(name, value);
        }
    };
    add(header::ACCEPT, "application/vnd.github+json");
    add(
        header::USER_AGENT,
        concat!("geekcli/", env!("CARGO_PKG_VERSION")),
    );
    if let Some(token) = token {
        add(header::AUTHORIZATION, &format!("Bearer {token}"));
    }
    let response = UreqClient::default()
        .get(&url, &headers, Some(Duration::from_secs(30)))
        .map_err(|e| match e {
            HttpError::NotFound { .. } => format!(
                "GitHub has no build-provenance attestation for an archive with sha256 {digest}"
            ),
            HttpError::RateLimited { .. } => {
                "GitHub is rate limiting attestation lookups from this address; set GH_TOKEN or retry later".into()
            }
            other => format!("cannot look up its attestation at {url}: {}", chain(&other)),
        })?;
    serde_json::from_reader(response.body())
        .map_err(|e| format!("unreadable attestation response from {url}: {e}"))
}

/// An error and its causes, `a: b: c`; reqwest's own message alone is only
/// "error sending request".
fn chain(err: &dyn std::error::Error) -> String {
    let mut text = err.to_string();
    let mut source = err.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

/// Check a `GET /repos/{repo}/attestations/sha256:{digest}` response: some
/// bundle in it must be an SLSA provenance statement naming this digest,
/// signed by [`SIGNER`], with a certificate chain and transparency-log entry
/// that verify against the embedded Sigstore trust root.
pub fn verify(attestations: &Value, digest: &str) -> Result<(), String> {
    verify_signed_by(attestations, digest, SIGNER)
}

fn verify_signed_by(attestations: &Value, digest: &str, signer: &str) -> Result<(), String> {
    ring_backend::install();
    let digest_hash =
        Sha256Hash::from_hex(digest).map_err(|e| format!("bad sha256 {digest}: {e}"))?;
    let root = TrustedRoot::from_embedded(SigstoreInstance::PublicGood)
        .map_err(|e| format!("cannot load Sigstore's trust root: {e}"))?;
    let verifier =
        Verifier::new(&root).map_err(|e| format!("cannot load Sigstore's trust root: {e}"))?;
    let policy = VerificationPolicy::new(IdentityMatcher::Uri(signer.into()), ISSUER);

    let bundles: Vec<&Value> = attestations
        .get("attestations")
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(|a| a.get("bundle")).collect())
        .unwrap_or_default();
    let mut last = "the response held no attestation bundles".to_string();
    for bundle in bundles {
        if predicate_type(bundle).as_deref() != Some(PREDICATE) {
            last = "the attestation is not build provenance".into();
            continue;
        }
        let parsed = match Bundle::from_json(&bundle.to_string()) {
            Ok(parsed) => parsed,
            Err(e) => {
                last = format!("unreadable attestation bundle: {e}");
                continue;
            }
        };
        match verifier.verify(digest_hash, &parsed, &policy) {
            Ok(_) => return Ok(()),
            Err(e) => last = format!("the attestation did not verify: {e}"),
        }
    }
    Err(last)
}

/// The in-toto statement's `predicateType`, read from the DSSE payload.
/// (The signature over the payload is what `verify` checks.)
fn predicate_type(bundle: &Value) -> Option<String> {
    let payload = bundle.pointer("/dsseEnvelope/payload")?.as_str()?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(payload)
        .ok()?;
    let statement: Value = serde_json::from_slice(&bytes).ok()?;
    statement.get("predicateType")?.as_str().map(str::to_string)
}

/// A verification-only `sigstore_crypto::CryptoProvider` over ring: SHA-2
/// digests and signature checks over whole messages, which is everything a
/// DSSE attestation needs (certificate chain, SCT, Rekor entry, envelope).
/// Checking a signature over a bare digest (`hashedrekord` bundles) is not
/// something ring offers, so it fails closed, as do ML-DSA keys.
mod ring_backend {
    use ring::digest::{self, Context, SHA256, SHA384, SHA512};
    use ring::signature::{self, UnparsedPublicKey, VerificationAlgorithm};
    use sigstore_crypto::provider::{CryptoProvider, HashContext};
    use sigstore_crypto::{Error, Result, SigningScheme};
    use sigstore_types::HashAlgorithm;

    #[derive(Debug)]
    struct Ring;

    static RING: Ring = Ring;

    /// Make ring the process's sigstore backend. Idempotent.
    pub fn install() {
        // Err only means a backend is already installed: this one, earlier.
        let _ = sigstore_crypto::provider::install_default_provider(&RING);
    }

    struct Hasher(Context);

    impl HashContext for Hasher {
        fn update(&mut self, data: &[u8]) {
            self.0.update(data);
        }

        fn finalize(self: Box<Self>) -> Vec<u8> {
            self.0.finish().as_ref().to_vec()
        }
    }

    fn algorithm(scheme: SigningScheme) -> Option<&'static dyn VerificationAlgorithm> {
        Some(match scheme {
            SigningScheme::EcdsaP256Sha256 => &signature::ECDSA_P256_SHA256_ASN1,
            SigningScheme::EcdsaP256Sha384 => &signature::ECDSA_P256_SHA384_ASN1,
            SigningScheme::EcdsaP384Sha256 => &signature::ECDSA_P384_SHA256_ASN1,
            SigningScheme::EcdsaP384Sha384 => &signature::ECDSA_P384_SHA384_ASN1,
            SigningScheme::Ed25519 => &signature::ED25519,
            SigningScheme::RsaPssSha256 => &signature::RSA_PSS_2048_8192_SHA256,
            SigningScheme::RsaPssSha384 => &signature::RSA_PSS_2048_8192_SHA384,
            SigningScheme::RsaPssSha512 => &signature::RSA_PSS_2048_8192_SHA512,
            SigningScheme::RsaPkcs1Sha256 => &signature::RSA_PKCS1_2048_8192_SHA256,
            SigningScheme::RsaPkcs1Sha384 => &signature::RSA_PKCS1_2048_8192_SHA384,
            SigningScheme::RsaPkcs1Sha512 => &signature::RSA_PKCS1_2048_8192_SHA512,
            _ => return None,
        })
    }

    impl CryptoProvider for Ring {
        fn hash(&self, algorithm: HashAlgorithm) -> Result<Box<dyn HashContext>> {
            let backend = match algorithm {
                HashAlgorithm::Sha2256 => &SHA256,
                HashAlgorithm::Sha2384 => &SHA384,
                HashAlgorithm::Sha2512 => &SHA512,
                other => {
                    return Err(Error::UnsupportedAlgorithm(format!(
                        "cannot compute {other} digests"
                    )))
                }
            };
            Ok(Box::new(Hasher(Context::new(backend))))
        }

        fn sha256(&self, data: &[u8]) -> [u8; 32] {
            let mut out = [0; 32];
            out.copy_from_slice(digest::digest(&SHA256, data).as_ref());
            out
        }

        fn sha384(&self, data: &[u8]) -> [u8; 48] {
            let mut out = [0; 48];
            out.copy_from_slice(digest::digest(&SHA384, data).as_ref());
            out
        }

        fn sha512(&self, data: &[u8]) -> [u8; 64] {
            let mut out = [0; 64];
            out.copy_from_slice(digest::digest(&SHA512, data).as_ref());
            out
        }

        fn verify_raw(
            &self,
            scheme: SigningScheme,
            spki_pubkey_bytes: &[u8],
            data: &[u8],
            signature: &[u8],
        ) -> Result<()> {
            let algorithm = algorithm(scheme).ok_or_else(|| {
                Error::UnsupportedAlgorithm(format!("{} is not supported", scheme.name()))
            })?;
            UnparsedPublicKey::new(algorithm, spki_pubkey_bytes)
                .verify(data, signature)
                .map_err(|_| Error::Verification(format!("{} signature invalid", scheme.name())))
        }

        fn verify_prehashed(
            &self,
            scheme: SigningScheme,
            _spki_pubkey_bytes: &[u8],
            _digest: &[u8],
            _signature: &[u8],
        ) -> Result<()> {
            Err(Error::UnsupportedAlgorithm(format!(
                "{} over a precomputed digest is not supported",
                scheme.name()
            )))
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn hashes_and_one_shot_digests_match_for_sha2_variants() -> Result<()> {
            let cases = [
                (HashAlgorithm::Sha2256, RING.sha256(b"hello").to_vec()),
                (HashAlgorithm::Sha2384, RING.sha384(b"hello").to_vec()),
                (HashAlgorithm::Sha2512, RING.sha512(b"hello").to_vec()),
            ];
            for (alg, expected) in cases {
                let mut h = RING.hash(alg)?;
                h.update(b"hel");
                h.update(b"lo");
                assert_eq!(h.finalize(), expected);
            }
            Ok(())
        }

        #[test]
        fn maps_supported_schemes_and_rejects_bad_or_prehashed_signatures() {
            for scheme in [
                SigningScheme::EcdsaP256Sha256,
                SigningScheme::EcdsaP256Sha384,
                SigningScheme::EcdsaP384Sha256,
                SigningScheme::EcdsaP384Sha384,
                SigningScheme::Ed25519,
                SigningScheme::RsaPssSha256,
                SigningScheme::RsaPssSha384,
                SigningScheme::RsaPssSha512,
                SigningScheme::RsaPkcs1Sha256,
                SigningScheme::RsaPkcs1Sha384,
                SigningScheme::RsaPkcs1Sha512,
            ] {
                assert!(algorithm(scheme).is_some(), "{scheme:?}");
            }
            assert!(matches!(
                RING.verify_raw(SigningScheme::EcdsaP256Sha256, b"not-a-key", b"msg", b"sig"),
                Err(Error::Verification(_))
            ));
            assert!(algorithm(SigningScheme::MlDsa65).is_none());
            assert!(matches!(
                RING.verify_raw(SigningScheme::MlDsa65, b"key", b"msg", b"sig"),
                Err(Error::UnsupportedAlgorithm(_))
            ));
            assert!(matches!(
                RING.verify_prehashed(SigningScheme::EcdsaP256Sha256, b"key", b"digest", b"sig"),
                Err(Error::UnsupportedAlgorithm(_))
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real attestation of the v0.12.1 archives, as GitHub serves it
    /// (minus its short-lived `bundle_url`). Verification uses the log's
    /// integrated time, so the fixture does not expire.
    const V0_12_1: &str = include_str!("../testdata/attestation-v0.12.1.json");
    const MACOS_ARM: &str = "abc70083b503abd91b54b213ae3f1040027d4071ba39b146dcf04c9f1100c879";
    const LINUX_X86: &str = "0784e85429a607f5fe0fda71e97ad21845811071b0d247de4cf416cb4456a504";

    fn fixture() -> std::result::Result<Value, serde_json::Error> {
        serde_json::from_str(V0_12_1)
    }

    #[test]
    fn a_release_archive_verifies() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let doc = fixture()?;
        assert_eq!(verify(&doc, MACOS_ARM), Ok(()));
        // one attestation covers every archive of the release
        assert_eq!(verify(&doc, LINUX_X86), Ok(()));
        Ok(())
    }

    #[test]
    fn only_the_release_workflow_on_main_is_trusted(
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let doc = fixture()?;
        for signer in [
            "https://github.com/RealGeeks/geekcli/.github/workflows/release.yml@refs/heads/other",
            "https://github.com/someone/geekcli/.github/workflows/release.yml@refs/heads/main",
        ] {
            let err = verify_signed_by(&doc, MACOS_ARM, signer).err();
            assert!(
                err.as_deref().is_some_and(|e| e.contains("identity")),
                "{signer}: {err:?}"
            );
        }
        Ok(())
    }

    #[test]
    fn another_digest_does_not() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let other = "1".repeat(64);
        let err = verify(&fixture()?, &other).err();
        assert!(
            err.as_deref().is_some_and(|e| e.contains("does not match")),
            "{err:?}"
        );
        Ok(())
    }

    #[test]
    fn a_tampered_statement_does_not() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let mut doc = fixture()?;
        let payload = doc
            .pointer("/attestations/0/bundle/dsseEnvelope/payload")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or("fixture has no payload")?;
        let engine = base64::engine::general_purpose::STANDARD;
        let text = String::from_utf8(engine.decode(payload)?)?;
        // claim a different archive digest in the signed statement
        let forged = text.replace(MACOS_ARM, &"2".repeat(64));
        assert_ne!(forged, text);
        if let Some(p) = doc.pointer_mut("/attestations/0/bundle/dsseEnvelope/payload") {
            *p = Value::String(engine.encode(forged));
        }
        let err = verify(&doc, &"2".repeat(64)).err();
        assert!(
            err.as_deref().is_some_and(|e| e.contains("did not verify")),
            "{err:?}"
        );
        Ok(())
    }

    #[test]
    fn nothing_to_verify_is_a_failure() {
        assert!(verify(&serde_json::json!({"attestations": []}), MACOS_ARM).is_err());
        assert!(verify(&serde_json::json!({}), MACOS_ARM).is_err());
    }

    #[test]
    fn hashes_a_file() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("a");
        std::fs::write(&path, b"hello")?;
        assert_eq!(
            sha256_file(&path)?,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        assert!(sha256_file(&dir.path().join("missing")).is_err());
        Ok(())
    }

    #[test]
    fn verify_rejects_bad_digest_wrong_predicate_and_malformed_bundle() {
        let bad_hex = verify(&serde_json::json!({}), "not-hex");
        assert!(bad_hex.as_ref().is_err_and(|e| e.contains("bad sha256")));

        let engine = base64::engine::general_purpose::STANDARD;
        for malformed in [
            serde_json::json!({}),
            serde_json::json!({"dsseEnvelope": {"payload": 123}}),
            serde_json::json!({"dsseEnvelope": {"payload": "!!!not-base64"}}),
            serde_json::json!({"dsseEnvelope": {"payload": engine.encode("not-json")}}),
            serde_json::json!({"dsseEnvelope": {"payload": engine.encode("{}")}}),
        ] {
            assert_eq!(predicate_type(&malformed), None, "{malformed}");
        }
        let wrong_pred = serde_json::json!({
            "attestations": [{
                "bundle": {
                    "dsseEnvelope": {
                        "payload": engine.encode(r#"{"predicateType":"https://example.com/other/v1"}"#)
                    }
                }
            }]
        });
        let err = verify(&wrong_pred, MACOS_ARM);
        assert!(err
            .as_ref()
            .is_err_and(|e| e.contains("not build provenance")));

        let malformed_bundle = serde_json::json!({
            "attestations": [{
                "bundle": {
                    "dsseEnvelope": {
                        "payload": engine.encode(format!(r#"{{"predicateType":"{PREDICATE}"}}"#))
                    }
                }
            }]
        });
        let err = verify(&malformed_bundle, MACOS_ARM);
        assert!(err
            .as_ref()
            .is_err_and(|e| e.contains("unreadable attestation bundle")));
    }

    #[test]
    fn check_fetches_verifies_with_token_and_maps_http_and_json_errors() {
        let mut server = mockito::Server::new();
        let path = format!("/repos/{REPO}/attestations/sha256:{MACOS_ARM}");

        let ok_mock = server
            .mock("GET", path.as_str())
            .match_header("authorization", "Bearer ghp_test_token")
            .match_header("accept", "application/vnd.github+json")
            .with_status(200)
            .with_body(V0_12_1)
            .create();
        assert_eq!(
            check(&server.url(), Some("ghp_test_token"), MACOS_ARM),
            Ok(())
        );
        ok_mock.assert();

        let not_found = server
            .mock("GET", path.as_str())
            .with_status(404)
            .with_body("{}")
            .create();
        let err = check(&server.url(), None, MACOS_ARM);
        assert!(err
            .as_ref()
            .is_err_and(|e| e.contains("no build-provenance attestation")));
        not_found.assert();

        let rate_limited = server
            .mock("GET", path.as_str())
            .with_status(429)
            .with_body("{}")
            .create();
        let err = check(&server.url(), None, MACOS_ARM);
        assert!(err.as_ref().is_err_and(|e| e.contains("rate limiting")));
        rate_limited.assert();

        let server_err = server
            .mock("GET", path.as_str())
            .with_status(500)
            .with_body("boom")
            .create();
        let err = check(&server.url(), None, MACOS_ARM);
        assert!(err
            .as_ref()
            .is_err_and(|e| e.contains("cannot look up its attestation")));
        server_err.assert();

        let bad_json = server
            .mock("GET", path.as_str())
            .with_status(200)
            .with_body("not json")
            .create();
        let err = check(&server.url(), None, MACOS_ARM);
        assert!(err
            .as_ref()
            .is_err_and(|e| e.contains("unreadable attestation response")));
        bad_json.assert();
    }

    #[test]
    fn chain_formats_nested_error_sources() {
        #[derive(Debug)]
        struct Outer(std::io::Error);
        impl std::fmt::Display for Outer {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "outer")
            }
        }
        impl std::error::Error for Outer {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                Some(&self.0)
            }
        }
        let err = Outer(std::io::Error::other("inner"));
        assert_eq!(chain(&err), "outer: inner");
    }
}
