//! Persistent client identity.
//!
//! A self-signed certificate kept in the [config dir](crate::config::config_dir)
//! gives this install a stable handle across runs. It's presented as the
//! lobby websocket's TLS client certificate (mTLS), so the lobby server can
//! read its SHA-256 fingerprint off the TLS handshake, derive a stable friend
//! code from it, and recognize the same install session to session — no
//! account system to ship.
//!
//! The cert + key are minted once (first launch, or whenever the files go
//! missing) and stored as PEM. [`load`] is called once at startup; the result
//! lives in `App` state and is threaded into each connect. It's best-effort:
//! any failure degrades to "no client identity" and the connection still
//! succeeds. The server only asks for a client cert when mTLS is enabled on the
//! endpoint, so an absent identity is also the steady state when mTLS is off —
//! sending one is always safe, and not sending one never breaks the dial.

use sha2::{Digest, Sha256};

/// Certificate file (PEM) under the config dir.
const CERT_FILE: &str = "identity.pem";
/// Private-key file (PEM) under the config dir.
const KEY_FILE: &str = "identity.key.pem";

/// Load the persistent client identity, minting + persisting a fresh
/// self-signed cert on first run. Returns `None` (logged) if it couldn't be
/// loaded or created. Call this once at startup and hold the result in app
/// state — it's threaded into each signaling connect, not re-read per dial.
pub fn load() -> Option<tango_lobby::ClientIdentity> {
    match load_or_create() {
        Ok((identity, fingerprint)) => {
            log::info!("client identity loaded (sha256 fingerprint: {fingerprint})");
            Some(identity)
        }
        Err(e) => {
            log::warn!("could not load or create client identity, connecting without one: {e:#}");
            None
        }
    }
}

/// Load the identity from disk, minting + persisting a fresh self-signed cert
/// when either file is missing. Returns the identity plus its lowercase-hex
/// SHA-256 fingerprint (of the DER certificate) for logging.
fn load_or_create() -> anyhow::Result<(tango_lobby::ClientIdentity, String)> {
    // `TANGO_IDENTITY_DIR` overrides where the identity files live, so a second
    // instance on the same machine can run a distinct identity (= a distinct
    // friend code) for local testing: `TANGO_IDENTITY_DIR=/tmp/tango-id2 cargo run`.
    let dir = match std::env::var_os("TANGO_IDENTITY_DIR") {
        Some(d) => {
            let dir = std::path::PathBuf::from(d);
            log::info!("using identity dir override: {}", dir.display());
            dir
        }
        None => crate::config::config_dir().ok_or_else(|| anyhow::anyhow!("no config dir"))?,
    };
    let cert_path = dir.join(CERT_FILE);
    let key_path = dir.join(KEY_FILE);

    let (cert_pem, key_pem) = match (std::fs::read_to_string(&cert_path), std::fs::read_to_string(&key_path)) {
        (Ok(cert), Ok(key)) => (cert, key),
        // Either file missing (or unreadable) → regenerate the pair. Treating a
        // half-present pair as "regenerate both" keeps cert and key in lockstep.
        _ => generate(&dir, &cert_path, &key_path)?,
    };

    let cert_der = parse_single_cert(&cert_pem)?;
    let key_der = parse_single_key(&key_pem)?;
    let fingerprint = hex(Sha256::digest(&cert_der).as_slice());
    Ok((tango_lobby::ClientIdentity { cert_der, key_der }, fingerprint))
}

/// Mint a fresh self-signed certificate, write both PEM files (the key locked
/// to owner-only where the platform supports it), and return their PEM text.
fn generate(
    dir: &std::path::Path,
    cert_path: &std::path::Path,
    key_path: &std::path::Path,
) -> anyhow::Result<(String, String)> {
    std::fs::create_dir_all(dir)?;
    // The CN is cosmetic for a self-signed identity cert — the SHA-256
    // fingerprint is the actual identity. rcgen's default is a P-256 ECDSA key,
    // serialized below as PKCS#8.
    let cert = rcgen::generate_simple_self_signed(vec!["tango-client".to_string()])?;
    let cert_pem = cert.serialize_pem()?;
    let key_pem = cert.serialize_private_key_pem();
    // Key first: if writing the cert fails we'd rather have an orphan key (next
    // launch sees the cert missing and regenerates both) than an orphan cert.
    write_private(key_path, key_pem.as_bytes())?;
    std::fs::write(cert_path, cert_pem.as_bytes())?;
    Ok((cert_pem, key_pem))
}

/// Write private-key material, restricting it to the owner on Unix. Best
/// effort: platforms without Unix permissions fall back to a plain write.
fn write_private(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write as _;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        f.write_all(bytes)
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, bytes)
    }
}

/// Pull the single DER certificate out of a PEM blob.
fn parse_single_cert(pem: &str) -> anyhow::Result<Vec<u8>> {
    let mut reader = std::io::BufReader::new(pem.as_bytes());
    rustls_pemfile::certs(&mut reader)?
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("no certificate in identity PEM"))
}

/// Pull the single DER private key out of a PEM blob. rcgen writes its default
/// ECDSA key as PKCS#8 ("PRIVATE KEY"), which is what `pkcs8_private_keys`
/// reads.
fn parse_single_key(pem: &str) -> anyhow::Result<Vec<u8>> {
    let mut reader = std::io::BufReader::new(pem.as_bytes());
    rustls_pemfile::pkcs8_private_keys(&mut reader)?
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("no PKCS#8 private key in identity PEM"))
}

/// Lowercase hex, no separators — the SHA-256 of the DER certificate.
fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}
