// SPDX-License-Identifier: GPL-3.0-or-later

//! Client-side file-transfer primitives.
//!
//! Two halves of the same problem:
//!
//! - [`send_file_through`] chops a local file into [`FileChunkFrame`]
//!   pieces of [`FILE_CHUNK_BYTES`] bytes each (base64-encoded), every
//!   chunk carrying the file's SHA-256 + total size so a late-joiner
//!   on the receiver side can still verify the assembly.
//! - [`FileReceiver`] accumulates inbound chunks into a partial file
//!   under the user's downloads dir, and on the final chunk verifies
//!   the SHA-256 and moves the assembled file into place.
//!
//! Wire format is documented in `PROTOCOL.md` §5 (added in v0.3.0 of
//! the protocol). The hub itself stays content-blind — chunks fan out
//! to peers exactly like text/image clips, just on a separate
//! per-client channel so a slow file doesn't head-of-line-block clips.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;
use tracing::{info, trace, warn};
use uuid::Uuid;

use crate::protocol::{FileChunkFrame, FILE_CHUNK_BYTES};

/// Send a single local file to the connected hub for fan-out. Reads
/// the file in 4-MiB chunks, base64-encodes each one, and pushes the
/// resulting [`FileChunkFrame`]s through the transport's outbound
/// file channel.
pub async fn send_file_through(
    path: &Path,
    outbound_files_tx: &mpsc::Sender<FileChunkFrame>,
) -> Result<()> {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow!("path has no filename component"))?
        .to_string();

    let metadata = fs::metadata(path).with_context(|| format!("stat {}", path.display()))?;
    let file_size = metadata.len();
    if file_size == 0 {
        anyhow::bail!("refusing to send a zero-byte file ({})", path.display());
    }

    let total_chunks = file_size.div_ceil(FILE_CHUNK_BYTES as u64);
    let total_chunks = u32::try_from(total_chunks)
        .map_err(|_| anyhow!("file too large to chunk in u32 — refusing"))?;

    let file_sha256 =
        sha256_of_path(path).with_context(|| format!("hashing {}", path.display()))?;
    let content_type = mime_for(path);
    let file_id = Uuid::new_v4();

    let mut reader = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut buf = vec![0u8; FILE_CHUNK_BYTES];
    let mut index = 0u32;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        let chunk = FileChunkFrame {
            file_id,
            name: name.clone(),
            content_type: content_type.clone(),
            chunk_index: index,
            total_chunks,
            file_sha256: file_sha256.clone(),
            file_size,
            payload_b64: STANDARD.encode(&buf[..n]),
            from: None,
        };
        outbound_files_tx
            .send(chunk)
            .await
            .map_err(|_| anyhow!("transport's outbound-files channel closed"))?;
        index += 1;
    }
    info!(
        file = %path.display(),
        file_id = %file_id,
        total_chunks = total_chunks,
        size = file_size,
        "file sent"
    );
    Ok(())
}

/// Best-effort MIME for a file from its extension. We don't want a heavy
/// dep for this — the receiver decides what to do with the bytes either
/// way; the content_type is metadata.
fn mime_for(path: &Path) -> String {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase());
    match ext.as_deref() {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("pdf") => "application/pdf",
        Some("zip") => "application/zip",
        Some("tar") => "application/x-tar",
        Some("gz" | "tgz") => "application/gzip",
        Some("txt" | "md" | "log") => "text/plain; charset=utf-8",
        Some("json") => "application/json",
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("mp4") => "video/mp4",
        Some("mov") => "video/quicktime",
        Some("mp3") => "audio/mpeg",
        Some("wav") => "audio/wav",
        _ => "application/octet-stream",
    }
    .to_string()
}

fn sha256_of_path(path: &Path) -> Result<String> {
    let mut f = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(fingerprint_hex(&hasher.finalize()))
}

fn fingerprint_hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

/// Receiver state: one in-flight `Transfer` per `file_id` in progress.
pub struct FileReceiver {
    save_dir: PathBuf,
    transfers: HashMap<Uuid, Transfer>,
}

struct Transfer {
    name: String,
    /// `<save_dir>/<file_id>.partial` — chunks are seek-written here as
    /// they arrive. On the final chunk we verify SHA-256 and rename.
    temp_path: PathBuf,
    file: File,
    total_chunks: u32,
    received: Vec<bool>,
    expected_sha256: String,
    expected_size: u64,
}

impl FileReceiver {
    /// Receiver writing to the platform default download dir
    /// (`~/Downloads/clipboardwire/` on Linux + macOS, the equivalent
    /// on Windows). Created on first use.
    pub fn new() -> Result<Self> {
        Self::with_save_dir(default_save_dir()?)
    }

    pub fn with_save_dir(save_dir: PathBuf) -> Result<Self> {
        fs::create_dir_all(&save_dir)
            .with_context(|| format!("creating {}", save_dir.display()))?;
        Ok(Self {
            save_dir,
            transfers: HashMap::new(),
        })
    }

    pub fn save_dir(&self) -> &Path {
        &self.save_dir
    }

    /// Process one inbound chunk. Returns `Ok(Some(path))` when the
    /// chunk completed an in-flight transfer, `Ok(None)` if more
    /// chunks are still pending for this `file_id`. Verifies SHA-256
    /// on completion and removes the partial file on mismatch.
    pub fn receive_chunk(&mut self, chunk: FileChunkFrame) -> Result<Option<PathBuf>> {
        if let Err(reason) = chunk.validate() {
            anyhow::bail!("rejecting inbound file_chunk: {reason}");
        }
        let bytes = STANDARD
            .decode(&chunk.payload_b64)
            .context("decoding chunk payload base64")?;

        let file_id = chunk.file_id;
        let total_chunks = chunk.total_chunks as usize;
        let chunk_index = chunk.chunk_index as usize;

        // First-chunk-of-this-file ⇒ create the partial file lazily.
        if !self.transfers.contains_key(&file_id) {
            let safe_name = sanitize_filename(&chunk.name);
            let temp_path = self.save_dir.join(format!("{file_id}.partial"));
            let file = OpenOptions::new()
                .create(true)
                .read(true)
                .write(true)
                .truncate(true)
                .open(&temp_path)
                .with_context(|| format!("creating {}", temp_path.display()))?;
            file.set_len(chunk.file_size).with_context(|| {
                format!(
                    "pre-allocating {} bytes for {}",
                    chunk.file_size,
                    temp_path.display()
                )
            })?;
            self.transfers.insert(
                file_id,
                Transfer {
                    name: safe_name,
                    temp_path,
                    file,
                    total_chunks: chunk.total_chunks,
                    received: vec![false; total_chunks],
                    expected_sha256: chunk.file_sha256.clone(),
                    expected_size: chunk.file_size,
                },
            );
        }

        let transfer = self
            .transfers
            .get_mut(&file_id)
            .expect("just inserted if missing");

        if chunk.total_chunks != transfer.total_chunks
            || chunk.file_sha256 != transfer.expected_sha256
            || chunk.file_size != transfer.expected_size
        {
            anyhow::bail!(
                "inconsistent metadata across chunks of file {} (sender changed total/hash/size mid-transfer)",
                file_id
            );
        }

        if transfer.received[chunk_index] {
            trace!(
                file_id = %file_id,
                index = chunk_index,
                "duplicate chunk; ignoring"
            );
            return Ok(None);
        }

        let offset = (chunk.chunk_index as u64) * (FILE_CHUNK_BYTES as u64);
        transfer.file.seek(SeekFrom::Start(offset))?;
        transfer.file.write_all(&bytes)?;
        transfer.received[chunk_index] = true;

        if !transfer.received.iter().all(|b| *b) {
            return Ok(None);
        }

        // Done — verify, rename, drop from map.
        transfer.file.sync_all()?;
        let expected = transfer.expected_sha256.clone();
        let actual = sha256_of_path(&transfer.temp_path)?;
        if actual != expected {
            warn!(
                file_id = %file_id,
                "SHA-256 mismatch (expected {expected}, got {actual}); removing partial file"
            );
            let _ = fs::remove_file(&transfer.temp_path);
            self.transfers.remove(&file_id);
            anyhow::bail!("file transfer failed SHA-256 verification");
        }

        let final_path = unique_path(&self.save_dir, &transfer.name);
        fs::rename(&transfer.temp_path, &final_path)?;
        let result_path = final_path.clone();
        info!(
            file = %result_path.display(),
            size = transfer.expected_size,
            "file transfer complete"
        );
        self.transfers.remove(&file_id);
        Ok(Some(result_path))
    }
}

fn default_save_dir() -> Result<PathBuf> {
    let base = directories::UserDirs::new().and_then(|u| u.download_dir().map(|p| p.to_path_buf()));
    let downloads = match base {
        Some(p) => p,
        None => {
            let base = directories::BaseDirs::new()
                .ok_or_else(|| anyhow!("could not locate the user's home directory"))?;
            base.home_dir().join("Downloads")
        }
    };
    Ok(downloads.join("clipboardwire"))
}

/// Strip path separators and dangerous characters. We don't try to be
/// clever about Unicode normalisation — we just refuse anything that
/// would let a malicious sender escape `save_dir`.
fn sanitize_filename(name: &str) -> String {
    let trimmed = name.trim();
    let sanitised: String = trimmed
        .chars()
        .map(|c| match c {
            '/' | '\\' | '\0' | ':' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    // Trailing-dot trim only — Windows refuses filenames that end in
    // `.`. Leading dots are fine (hidden-file convention on Unix) and
    // not exploitable once path separators have already been mapped.
    let cleaned = sanitised.trim_end_matches('.').to_string();
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        "received-file".to_string()
    } else {
        cleaned
    }
}

fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let stem = Path::new(name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(name);
    let ext = Path::new(name).extension().and_then(|s| s.to_str());
    for n in 1..10_000 {
        let with_n = match ext {
            Some(e) => format!("{stem} ({n}).{e}"),
            None => format!("{stem} ({n})"),
        };
        let candidate = dir.join(with_n);
        if !candidate.exists() {
            return candidate;
        }
    }
    // Fallback: use a uuid suffix if 10k collisions ever happen
    // (shouldn't, but we don't want an infinite loop).
    dir.join(format!("{name}-{}", Uuid::new_v4()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_dir(label: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "cw-filerecv-{label}-{}-{nanos}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn make_chunks(name: &str, content: &[u8]) -> Vec<FileChunkFrame> {
        let file_id = Uuid::nil();
        let total = content.len().div_ceil(FILE_CHUNK_BYTES.max(1)).max(1) as u32;
        let sha = {
            let mut h = Sha256::new();
            h.update(content);
            fingerprint_hex(&h.finalize())
        };
        let mut chunks = Vec::new();
        for (i, slice) in content.chunks(FILE_CHUNK_BYTES).enumerate() {
            chunks.push(FileChunkFrame {
                file_id,
                name: name.to_string(),
                content_type: "application/octet-stream".to_string(),
                chunk_index: i as u32,
                total_chunks: total,
                file_sha256: sha.clone(),
                file_size: content.len() as u64,
                payload_b64: STANDARD.encode(slice),
                from: None,
            });
        }
        chunks
    }

    #[test]
    fn single_chunk_transfer_writes_file_with_original_name() {
        let dir = unique_dir("single");
        let mut rx = FileReceiver::with_save_dir(dir.clone()).unwrap();

        let body = b"hello clipboardwire";
        let chunks = make_chunks("hello.txt", body);

        let mut final_path = None;
        for c in chunks {
            if let Some(p) = rx.receive_chunk(c).unwrap() {
                final_path = Some(p);
            }
        }
        let final_path = final_path.expect("transfer should have completed");
        assert_eq!(final_path.file_name().unwrap(), "hello.txt");
        let got = fs::read(&final_path).unwrap();
        assert_eq!(got, body);
    }

    #[test]
    fn multi_chunk_transfer_assembles_correctly() {
        let dir = unique_dir("multi");
        let mut rx = FileReceiver::with_save_dir(dir).unwrap();

        // 9 MiB of pseudo-random bytes ⇒ 3 chunks at 4 MiB each.
        let mut body = vec![0u8; 9 * 1024 * 1024];
        for (i, b) in body.iter_mut().enumerate() {
            *b = (i & 0xff) as u8;
        }
        let chunks = make_chunks("blob.bin", &body);
        assert!(chunks.len() >= 3, "this test needs >=3 chunks");

        let mut final_path = None;
        for c in chunks {
            if let Some(p) = rx.receive_chunk(c).unwrap() {
                final_path = Some(p);
            }
        }
        let final_path = final_path.expect("transfer should have completed");
        let got = fs::read(&final_path).unwrap();
        assert_eq!(got, body);
    }

    #[test]
    fn out_of_order_chunks_assemble_correctly() {
        let dir = unique_dir("ooo");
        let mut rx = FileReceiver::with_save_dir(dir).unwrap();

        let mut body = vec![0u8; 6 * 1024 * 1024];
        for (i, b) in body.iter_mut().enumerate() {
            *b = ((i * 7) & 0xff) as u8;
        }
        let mut chunks = make_chunks("ooo.bin", &body);
        chunks.reverse();
        let mut final_path = None;
        for c in chunks {
            if let Some(p) = rx.receive_chunk(c).unwrap() {
                final_path = Some(p);
            }
        }
        let final_path = final_path.expect("transfer should have completed");
        let got = fs::read(&final_path).unwrap();
        assert_eq!(got, body);
    }

    #[test]
    fn sha256_mismatch_fails_and_cleans_up_partial() {
        let dir = unique_dir("badsha");
        let mut rx = FileReceiver::with_save_dir(dir.clone()).unwrap();

        let body = b"original";
        let mut chunks = make_chunks("victim.txt", body);
        // Tamper with the first chunk's payload (keep length).
        chunks[0].payload_b64 = STANDARD.encode(b"tampered");

        let mut err = None;
        for c in chunks {
            if let Err(e) = rx.receive_chunk(c) {
                err = Some(e);
                break;
            }
        }
        assert!(err.is_some(), "tampered chunk should fail SHA verification");
        // No final file, no .partial leftover.
        let entries: Vec<_> = fs::read_dir(&dir).unwrap().collect();
        assert!(
            entries.is_empty() || entries.iter().all(|e| !e.as_ref().unwrap().path().exists()),
            "save_dir should be empty after a failed transfer"
        );
    }

    #[test]
    fn duplicate_chunk_is_ignored_not_errored() {
        let dir = unique_dir("dup");
        let mut rx = FileReceiver::with_save_dir(dir).unwrap();

        let body = b"once and again";
        let chunks = make_chunks("dup.txt", body);
        // Send first chunk twice — should not error.
        rx.receive_chunk(chunks[0].clone()).unwrap();
        rx.receive_chunk(chunks[0].clone()).unwrap();
        for c in chunks.into_iter().skip(1) {
            rx.receive_chunk(c).unwrap();
        }
    }

    #[test]
    fn sanitize_filename_strips_path_separators() {
        assert_eq!(sanitize_filename("../etc/passwd"), ".._etc_passwd");
        assert_eq!(sanitize_filename("normal.txt"), "normal.txt");
        assert_eq!(sanitize_filename("   "), "received-file");
        assert_eq!(sanitize_filename("foo\\bar"), "foo_bar");
    }

    #[test]
    fn unique_path_disambiguates_existing_filenames() {
        let dir = unique_dir("unique");
        let p = unique_path(&dir, "doc.txt");
        fs::write(&p, b"x").unwrap();
        let p2 = unique_path(&dir, "doc.txt");
        assert_ne!(p, p2);
        assert!(
            p2.file_name().unwrap().to_string_lossy().contains("(1)"),
            "second `doc.txt` should get a numeric suffix: {}",
            p2.display()
        );
    }
}
