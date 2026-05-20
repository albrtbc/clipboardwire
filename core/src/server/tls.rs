// SPDX-License-Identifier: GPL-3.0-or-later

//! Self-signed TLS cert auto-generation for the hub.
//!
//! When [`ServerConfig`](crate::server::ServerConfig) has no
//! `tls_cert_file`/`tls_key_file` set and `tls_disabled` is false, the
//! hub generates a self-signed cert and persists it under
//! `<state_dir>/self-signed.{crt,key}`. Subsequent restarts reload the
//! same pair, so client-side pinning via `tls_ca_file` stays stable.
//!
//! The cert's SANs cover `localhost`, `127.0.0.1`, `::1`, the bind IP
//! (if specific), and the machine's hostname — enough for the common
//! LAN-or-loopback case without needing extra configuration.
//!
//! Self-signed alone does **not** authenticate the hub to clients: they
//! still have to either pin the cert (set `tls_ca_file` to the saved
//! `.crt`) or skip verification (`tls_insecure = true`). Auto-gen only
//! moves the wire from plaintext to encrypted-but-unauthenticated. The
//! SHA-256 fingerprint is logged on first generation so the operator
//! can pin it from clients out-of-band.

use std::fs;
use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rcgen::{generate_simple_self_signed, CertifiedKey};
use sha2::{Digest, Sha256};
use tracing::info;

const CERT_FILENAME: &str = "self-signed.crt";
const KEY_FILENAME: &str = "self-signed.key";

/// Outcome of [`ensure_self_signed_cert`].
#[derive(Debug, Clone)]
pub struct EnsuredCert {
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
    /// Colon-separated uppercase hex SHA-256 of the DER cert — same
    /// format browsers and `openssl x509 -fingerprint -sha256` print.
    pub fingerprint_sha256: String,
    /// `true` if the cert was generated on this call, `false` if it was
    /// already on disk from a previous run.
    pub was_new: bool,
}

/// Return the cert+key paths under `state_dir`, generating + persisting
/// them on first call.
pub fn ensure_self_signed_cert(state_dir: &Path, bind: SocketAddr) -> Result<EnsuredCert> {
    let cert_path = state_dir.join(CERT_FILENAME);
    let key_path = state_dir.join(KEY_FILENAME);

    if cert_path.exists() && key_path.exists() {
        let cert_pem =
            fs::read(&cert_path).with_context(|| format!("reading {}", cert_path.display()))?;
        let fingerprint = fingerprint_of_pem(&cert_pem)
            .with_context(|| format!("computing fingerprint of {}", cert_path.display()))?;
        info!(
            cert = %cert_path.display(),
            sha256 = %fingerprint,
            "reusing existing self-signed TLS cert"
        );
        return Ok(EnsuredCert {
            cert_path,
            key_path,
            fingerprint_sha256: fingerprint,
            was_new: false,
        });
    }

    fs::create_dir_all(state_dir)
        .with_context(|| format!("creating state dir {}", state_dir.display()))?;
    let sans = subject_alt_names_for(bind);
    let CertifiedKey { cert, key_pair } =
        generate_simple_self_signed(sans.clone()).context("generating self-signed certificate")?;
    let cert_pem = cert.pem();
    let key_pem = key_pair.serialize_pem();

    // Cert is public, key is private. Both 0600 is safe overkill but
    // 0644/0600 lets a non-root remote-copy of the cert work without
    // perms gymnastics.
    write_file_with_mode(&cert_path, cert_pem.as_bytes(), 0o644)?;
    write_file_with_mode(&key_path, key_pem.as_bytes(), 0o600)?;

    let fingerprint = fingerprint_of_der(cert.der());
    info!(
        cert = %cert_path.display(),
        key = %key_path.display(),
        sha256 = %fingerprint,
        sans = ?sans,
        "generated self-signed TLS cert for the hub — pin this fingerprint on clients, \
         or set `tls_ca_file` to the cert file's path"
    );
    Ok(EnsuredCert {
        cert_path,
        key_path,
        fingerprint_sha256: fingerprint,
        was_new: true,
    })
}

/// SANs we put on the generated cert. The goal isn't completeness — the
/// user can always regenerate or BYO cert — it's "works out of the box
/// for the common 'one box on a LAN' case."
fn subject_alt_names_for(bind: SocketAddr) -> Vec<String> {
    let mut sans = vec![
        "localhost".to_string(),
        "127.0.0.1".to_string(),
        "::1".to_string(),
    ];

    let ip = bind.ip();
    if !ip.is_unspecified() && !ip.is_loopback() {
        sans.push(ip.to_string());
    }

    if let Ok(h) = hostname::get() {
        if let Some(h) = h.to_str() {
            let h = h.trim();
            if !h.is_empty() && !sans.iter().any(|s| s == h) {
                sans.push(h.to_string());
            }
        }
    }

    sans
}

fn write_file_with_mode(path: &Path, contents: &[u8], mode: u32) -> Result<()> {
    let _ = mode;
    let mut f = fs::File::create(path).with_context(|| format!("creating {}", path.display()))?;
    f.write_all(contents)
        .with_context(|| format!("writing {}", path.display()))?;
    f.sync_all().ok();
    drop(f);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path)?.permissions();
        perms.set_mode(mode);
        fs::set_permissions(path, perms)?;
    }
    Ok(())
}

fn fingerprint_of_der(der: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(der);
    let digest = hasher.finalize();
    digest
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

fn fingerprint_of_pem(pem: &[u8]) -> Result<String> {
    let der = first_pem_block_to_der(pem)?;
    Ok(fingerprint_of_der(&der))
}

/// Minimal PEM → DER decode for the first `BEGIN CERTIFICATE` block.
/// We avoid adding the `pem` crate as a direct dep just for this.
fn first_pem_block_to_der(pem: &[u8]) -> Result<Vec<u8>> {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;

    let s = std::str::from_utf8(pem).context("PEM is not UTF-8")?;
    let begin = s
        .find("-----BEGIN ")
        .context("no `-----BEGIN ` marker in PEM")?;
    let after_begin_line = s[begin..]
        .find('\n')
        .map(|i| begin + i + 1)
        .context("incomplete BEGIN line")?;
    let end_offset = s[after_begin_line..]
        .find("-----END ")
        .context("no `-----END ` marker in PEM")?;
    let body = &s[after_begin_line..after_begin_line + end_offset];
    let cleaned: String = body.chars().filter(|c| !c.is_whitespace()).collect();
    STANDARD
        .decode(cleaned.as_bytes())
        .context("decoding PEM base64 body")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_dir(label: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("cw-tls-{label}-{}-{nanos}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn generates_persists_and_reloads_with_same_fingerprint() {
        let dir = unique_dir("gen");
        let bind: SocketAddr = "192.168.3.7:8484".parse().unwrap();

        let first = ensure_self_signed_cert(&dir, bind).unwrap();
        assert!(first.was_new);
        assert!(first.cert_path.exists());
        assert!(first.key_path.exists());

        // The PEM body should be a non-empty BEGIN CERTIFICATE block.
        let cert_pem = std::fs::read_to_string(&first.cert_path).unwrap();
        assert!(cert_pem.contains("BEGIN CERTIFICATE"));
        assert!(cert_pem.contains("END CERTIFICATE"));

        let second = ensure_self_signed_cert(&dir, bind).unwrap();
        assert!(!second.was_new, "second call should reuse, not regenerate");
        assert_eq!(
            first.fingerprint_sha256, second.fingerprint_sha256,
            "fingerprint must be stable across reloads"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fingerprint_format_matches_openssl_style() {
        let fp = fingerprint_of_der(b"hello");
        // SHA-256 of "hello" =
        // 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824
        assert_eq!(
            fp,
            "2C:F2:4D:BA:5F:B0:A3:0E:26:E8:3B:2A:C5:B9:E2:9E:\
             1B:16:1E:5C:1F:A7:42:5E:73:04:33:62:93:8B:98:24"
                .replace(['\n', ' '], "")
        );
    }

    #[test]
    #[cfg(unix)]
    fn generated_key_file_is_0600() {
        use std::os::unix::fs::PermissionsExt;
        let dir = unique_dir("mode");
        let bind: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let out = ensure_self_signed_cert(&dir, bind).unwrap();
        let mode = std::fs::metadata(&out.key_path)
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bind_ip_is_added_to_sans_when_specific() {
        let bind: SocketAddr = "192.168.1.42:8484".parse().unwrap();
        let sans = subject_alt_names_for(bind);
        assert!(sans.iter().any(|s| s == "192.168.1.42"));
        assert!(sans.iter().any(|s| s == "localhost"));
    }

    #[test]
    fn unspecified_bind_does_not_add_zeroes_to_sans() {
        let bind: SocketAddr = "0.0.0.0:8484".parse().unwrap();
        let sans = subject_alt_names_for(bind);
        assert!(!sans.iter().any(|s| s == "0.0.0.0"));
    }
}
