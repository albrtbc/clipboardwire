// SPDX-License-Identifier: GPL-3.0-or-later

//! Wire protocol types. See `PROTOCOL.md` for the full spec.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Value advertised by the server in the `welcome` frame. Bumped to
/// 0.3 with the introduction of `file_chunk` frames; older clients that
/// don't understand them will treat them as unknown and ignore.
pub const PROTOCOL_VERSION: &str = "clipboardwire/0.3.0";

/// Maximum WebSocket frame size accepted by the server (50 MiB) — large
/// enough for typical screenshot-sized PNGs without operator tuning,
/// and for the per-chunk size used by file transfers (4 MiB raw →
/// ~5.5 MiB base64 → plenty of headroom).
pub const MAX_FRAME_BYTES: usize = 50 * 1024 * 1024;

/// Bytes per `file_chunk` frame (raw, pre-base64). Chunks are independent
/// JSON frames; the receiver assembles them by `file_id` + `chunk_index`.
/// 4 MiB strikes a balance between per-frame memory peaks, total chunk
/// count for big files, and reconnect-recovery granularity. With base64
/// inflation a chunk's wire size is roughly 5.5 MiB, well inside
/// [`MAX_FRAME_BYTES`].
pub const FILE_CHUNK_BYTES: usize = 4 * 1024 * 1024;

/// `content_type` for UTF-8 text payloads.
pub const TEXT_CONTENT_TYPE: &str = "text/plain; charset=utf-8";
/// `content_type` for PNG image payloads.
pub const IMAGE_PNG_CONTENT_TYPE: &str = "image/png";

/// Back-compat alias kept while we migrate callers. Prefer
/// [`TEXT_CONTENT_TYPE`] in new code.
pub const SUPPORTED_CONTENT_TYPE: &str = TEXT_CONTENT_TYPE;

/// Returns whether `content_type` is one of the textual MIME types whose
/// payload travels in the `content` field rather than `content_b64`.
pub fn is_text_content_type(content_type: &str) -> bool {
    content_type.starts_with("text/")
}

/// `clip` frame body. Either `content` (text) or `content_b64` (binary) is
/// set; never both, never neither — see [`ClipFrame::validate`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClipFrame {
    pub id: Uuid,
    pub ts: i64,
    pub content_type: String,
    /// Text payload (UTF-8 string). Present iff `content_type` is textual.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Base64-encoded binary payload. Present iff `content_type` is binary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_b64: Option<String>,
    /// Filled by the server on relay; absent on the client's outbound send.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<Uuid>,
}

impl ClipFrame {
    /// Returns `Err` if neither or both payload fields are set, or if the
    /// payload kind disagrees with `content_type`.
    pub fn validate(&self) -> Result<(), &'static str> {
        match (self.content.is_some(), self.content_b64.is_some()) {
            (true, true) => Err("clip frame has both `content` and `content_b64`"),
            (false, false) => Err("clip frame has neither `content` nor `content_b64`"),
            (true, false) if !is_text_content_type(&self.content_type) => {
                Err("`content` requires a text/* content_type")
            }
            (false, true) if is_text_content_type(&self.content_type) => {
                Err("`content_b64` requires a non-text content_type")
            }
            _ => Ok(()),
        }
    }
}

/// `welcome` frame body.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WelcomeFrame {
    pub server: String,
    pub client_id: Uuid,
    pub last_clip: Option<ClipFrame>,
}

/// `error` frame body.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ErrorFrame {
    pub code: ErrorCode,
    pub message: String,
}

/// Protocol-level error codes — also the `name` column of close codes 4001–4005.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    Unauthorized,
    BadFrame,
    FrameTooLarge,
    UnsupportedType,
    DuplicateSession,
}

impl ErrorCode {
    /// WebSocket close code that pairs with this error (per `PROTOCOL.md` §6).
    pub fn close_code(self) -> u16 {
        match self {
            Self::Unauthorized => 4001,
            Self::BadFrame => 4002,
            Self::FrameTooLarge => 4003,
            Self::UnsupportedType => 4004,
            Self::DuplicateSession => 4005,
        }
    }
}

/// A single chunk of a file transfer.
///
/// One file is sent as a sequence of `file_chunk` frames sharing the
/// same `file_id`. The receiver assembles them into a temp file under
/// the receiver's downloads directory (see
/// `client::file::FileReceiver`), and on the final chunk verifies the
/// SHA-256 against the value carried in every chunk before moving the
/// finished file into place.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileChunkFrame {
    /// Unique transfer id. All chunks of the same file share this.
    pub file_id: Uuid,
    /// Original filename (no directory component). The receiver
    /// sanitises this before using it as a path.
    pub name: String,
    /// Best-effort MIME type. May be `application/octet-stream`.
    pub content_type: String,
    /// 0-based chunk index.
    pub chunk_index: u32,
    /// Total number of chunks for this file. Receivers know they're
    /// done assembling when they've received every index in
    /// `0..total_chunks`.
    pub total_chunks: u32,
    /// SHA-256 of the *entire* file as a colon-separated uppercase
    /// hex string, repeated in every chunk so a receiver that joins
    /// mid-transfer can still verify on completion.
    pub file_sha256: String,
    /// Total file size in bytes.
    pub file_size: u64,
    /// Base64-encoded bytes for this chunk. Decoded length is at most
    /// [`FILE_CHUNK_BYTES`] (smaller for the last chunk).
    pub payload_b64: String,
    /// Filled by the hub on relay; absent on the client's outbound send.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<Uuid>,
}

impl FileChunkFrame {
    /// Sanity-check fields a malicious sender could exploit. The hub
    /// calls this before relaying so a single bad client can't make us
    /// fan-out garbage.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.name.is_empty() {
            return Err("file_chunk name must not be empty");
        }
        if self.name.contains('\0') {
            return Err("file_chunk name must not contain NUL bytes");
        }
        if self.total_chunks == 0 {
            return Err("file_chunk total_chunks must be >= 1");
        }
        if self.chunk_index >= self.total_chunks {
            return Err("file_chunk chunk_index >= total_chunks");
        }
        if self.payload_b64.is_empty() {
            return Err("file_chunk payload_b64 must not be empty");
        }
        if self.file_sha256.is_empty() {
            return Err("file_chunk file_sha256 must not be empty");
        }
        Ok(())
    }
}

/// Any frame on the wire. `#[serde(tag = "type")]` flattens the variant fields
/// alongside a `"type": "<variant>"` discriminator, matching `PROTOCOL.md` §3.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Frame {
    Clip(ClipFrame),
    Welcome(WelcomeFrame),
    Error(ErrorFrame),
    FileChunk(FileChunkFrame),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_frame_text_roundtrip() {
        let id = Uuid::new_v4();
        let from = Uuid::new_v4();
        let f = Frame::Clip(ClipFrame {
            id,
            ts: 1_716_163_200_000,
            content_type: TEXT_CONTENT_TYPE.to_string(),
            content: Some("hello world".to_string()),
            content_b64: None,
            from: Some(from),
        });
        let s = serde_json::to_string(&f).unwrap();
        assert!(s.contains(r#""type":"clip""#));
        assert!(s.contains(r#""content":"hello world""#));
        assert!(!s.contains("content_b64"));
        let back: Frame = serde_json::from_str(&s).unwrap();
        assert_eq!(back, f);
    }

    #[test]
    fn clip_frame_binary_roundtrip() {
        let f = Frame::Clip(ClipFrame {
            id: Uuid::new_v4(),
            ts: 0,
            content_type: IMAGE_PNG_CONTENT_TYPE.to_string(),
            content: None,
            content_b64: Some("iVBORw0KGgo=".to_string()),
            from: None,
        });
        let s = serde_json::to_string(&f).unwrap();
        assert!(s.contains(r#""content_type":"image/png""#));
        assert!(s.contains(r#""content_b64":"iVBORw0KGgo=""#));
        assert!(!s.contains(r#""content":"#));
        let back: Frame = serde_json::from_str(&s).unwrap();
        assert_eq!(back, f);
    }

    #[test]
    fn clip_frame_validate_rejects_missing_payload() {
        let f = ClipFrame {
            id: Uuid::nil(),
            ts: 0,
            content_type: TEXT_CONTENT_TYPE.to_string(),
            content: None,
            content_b64: None,
            from: None,
        };
        assert!(f.validate().is_err());
    }

    #[test]
    fn clip_frame_validate_rejects_double_payload() {
        let f = ClipFrame {
            id: Uuid::nil(),
            ts: 0,
            content_type: TEXT_CONTENT_TYPE.to_string(),
            content: Some("x".into()),
            content_b64: Some("eA==".into()),
            from: None,
        };
        assert!(f.validate().is_err());
    }

    #[test]
    fn clip_frame_validate_rejects_mismatched_kind() {
        let text_with_b64 = ClipFrame {
            id: Uuid::nil(),
            ts: 0,
            content_type: TEXT_CONTENT_TYPE.to_string(),
            content: None,
            content_b64: Some("eA==".into()),
            from: None,
        };
        assert!(text_with_b64.validate().is_err());

        let bin_with_text = ClipFrame {
            id: Uuid::nil(),
            ts: 0,
            content_type: IMAGE_PNG_CONTENT_TYPE.to_string(),
            content: Some("x".into()),
            content_b64: None,
            from: None,
        };
        assert!(bin_with_text.validate().is_err());
    }

    #[test]
    fn welcome_frame_serializes_with_type_tag() {
        let f = Frame::Welcome(WelcomeFrame {
            server: PROTOCOL_VERSION.to_string(),
            client_id: Uuid::nil(),
            last_clip: None,
        });
        let s = serde_json::to_string(&f).unwrap();
        assert!(s.starts_with(r#"{"type":"welcome""#));
        assert!(s.contains(r#""last_clip":null"#));
    }

    #[test]
    fn error_codes_have_documented_close_codes() {
        assert_eq!(ErrorCode::Unauthorized.close_code(), 4001);
        assert_eq!(ErrorCode::BadFrame.close_code(), 4002);
        assert_eq!(ErrorCode::FrameTooLarge.close_code(), 4003);
        assert_eq!(ErrorCode::UnsupportedType.close_code(), 4004);
        assert_eq!(ErrorCode::DuplicateSession.close_code(), 4005);
    }

    #[test]
    fn unknown_type_fails_to_deserialize() {
        // §3: unknown frame types are ignored. We let serde reject them; the
        // ws handler will dispatch on the parse failure (see ws.rs).
        let s = r#"{"type":"future_frame","foo":1}"#;
        let r: Result<Frame, _> = serde_json::from_str(s);
        assert!(r.is_err());
    }

    #[test]
    fn file_chunk_frame_roundtrip() {
        let f = Frame::FileChunk(FileChunkFrame {
            file_id: Uuid::nil(),
            name: "doc.pdf".to_string(),
            content_type: "application/pdf".to_string(),
            chunk_index: 0,
            total_chunks: 3,
            file_sha256: "AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99:\
                 AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99"
                .replace(['\n', ' '], ""),
            file_size: 12_345_678,
            payload_b64: "iVBORw0KGgo=".to_string(),
            from: None,
        });
        let s = serde_json::to_string(&f).unwrap();
        assert!(s.contains(r#""type":"file_chunk""#));
        assert!(s.contains(r#""name":"doc.pdf""#));
        assert!(s.contains(r#""chunk_index":0"#));
        let back: Frame = serde_json::from_str(&s).unwrap();
        assert_eq!(back, f);
    }

    #[test]
    fn file_chunk_validate_rejects_traversal_in_name() {
        let mut chunk = base_chunk();
        chunk.name = "".into();
        assert!(chunk.validate().is_err());

        chunk.name = "ok\0sneaky".into();
        assert!(chunk.validate().is_err());
    }

    #[test]
    fn file_chunk_validate_rejects_out_of_range_index() {
        let mut chunk = base_chunk();
        chunk.chunk_index = 4;
        chunk.total_chunks = 4;
        assert!(chunk.validate().is_err());
    }

    #[test]
    fn file_chunk_validate_rejects_zero_total() {
        let mut chunk = base_chunk();
        chunk.total_chunks = 0;
        assert!(chunk.validate().is_err());
    }

    fn base_chunk() -> FileChunkFrame {
        FileChunkFrame {
            file_id: Uuid::nil(),
            name: "a.bin".into(),
            content_type: "application/octet-stream".into(),
            chunk_index: 0,
            total_chunks: 1,
            file_sha256: "00".into(),
            file_size: 1,
            payload_b64: "AA==".into(),
            from: None,
        }
    }

    #[test]
    fn clip_frame_from_field_is_omitted_when_none() {
        let f = ClipFrame {
            id: Uuid::nil(),
            ts: 0,
            content_type: TEXT_CONTENT_TYPE.to_string(),
            content: Some(String::new()),
            content_b64: None,
            from: None,
        };
        let s = serde_json::to_string(&f).unwrap();
        assert!(!s.contains("\"from\""));
    }
}
