// SPDX-License-Identifier: GPL-3.0-or-later

//! Wire protocol types. See `PROTOCOL.md` for the full spec.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Value advertised by the server in the `welcome` frame.
pub const PROTOCOL_VERSION: &str = "clipboardwire/0.2.0";

/// Maximum WebSocket frame size accepted by the server (50 MiB) — large
/// enough for typical screenshot-sized PNGs without operator tuning.
pub const MAX_FRAME_BYTES: usize = 50 * 1024 * 1024;

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

/// Any frame on the wire. `#[serde(tag = "type")]` flattens the variant fields
/// alongside a `"type": "<variant>"` discriminator, matching `PROTOCOL.md` §3.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Frame {
    Clip(ClipFrame),
    Welcome(WelcomeFrame),
    Error(ErrorFrame),
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
