use crate::identity::{hex, unix_now};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

pub const MAX_ATTACHMENT_SIZE: usize = 10 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttachmentError {
    FileTooLarge { size: usize, max: usize },
    FileEmpty,
    UnsupportedMimeType(String),
    MagicBytesMismatch { expected: String, actual: String },
    DisallowedExecutableOrScript,
    DangerousSvgContent,
    InvalidEncoding,
    CorruptedImageData,
}

impl std::fmt::Display for AttachmentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AttachmentError::FileTooLarge { size, max } => {
                write!(f, "File size {} bytes exceeds maximum {} bytes (10MB)", size, max)
            }
            AttachmentError::FileEmpty => write!(f, "Attachment file is empty"),
            AttachmentError::UnsupportedMimeType(m) => write!(f, "Unsupported attachment MIME type: {}", m),
            AttachmentError::MagicBytesMismatch { expected, actual } => {
                write!(f, "File signature mismatch (expected {}, got {})", expected, actual)
            }
            AttachmentError::DisallowedExecutableOrScript => {
                write!(f, "Executable binary or script attachments are strictly disallowed")
            }
            AttachmentError::DangerousSvgContent => {
                write!(f, "SVG contains disallowed embedded scripts or event handlers")
            }
            AttachmentError::InvalidEncoding => write!(f, "Text file is not valid UTF-8"),
            AttachmentError::CorruptedImageData => write!(f, "Image file data is corrupted or cannot be parsed"),
        }
    }
}

impl std::error::Error for AttachmentError {}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct AttachmentMeta {
    pub id: String,
    pub uploader_pubkey: String,
    pub filename: String,
    pub mime_type: String,
    pub size_bytes: usize,
    pub sha256: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub created_at: u64,
}

pub struct AttachmentValidator;

impl AttachmentValidator {
    pub fn sanitize_filename(raw_name: &str) -> String {
        let file_name = Path::new(raw_name)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("attachment");

        let mut sanitized = String::with_capacity(file_name.len());
        for c in file_name.chars() {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                sanitized.push(c);
            } else if c.is_whitespace() {
                sanitized.push('_');
            }
        }

        let trimmed = sanitized.trim_matches('.').to_string();
        if trimmed.is_empty() {
            "attachment".to_string()
        } else {
            trimmed
        }
    }

    pub fn compute_sha256(data: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data);
        hex(&hasher.finalize())
    }

    pub fn is_executable_or_script(data: &[u8]) -> bool {
        if data.len() < 2 {
            return false;
        }

        if data.len() >= 2 && data[0] == 0x4D && data[1] == 0x5A {
            return true;
        }

        if data.len() >= 4 && data[0] == 0x7F && data[1] == b'E' && data[2] == b'L' && data[3] == b'F' {
            return true;
        }

        if data.len() >= 4 {
            let magic = &data[0..4];
            if magic == [0xFE, 0xED, 0xFA, 0xCE]
                || magic == [0xCE, 0xFA, 0xED, 0xFE]
                || magic == [0xFE, 0xED, 0xFA, 0xCF]
                || magic == [0xCF, 0xFA, 0xED, 0xFE]
                || magic == [0xCA, 0xFE, 0xBA, 0xBE]
            {
                return true;
            }
        }

        if data.len() >= 2 && data[0] == b'#' && data[1] == b'!' {
            return true;
        }

        false
    }

    pub fn scan_svg_safety(data: &[u8]) -> Result<(), AttachmentError> {
        let text = std::str::from_utf8(data).map_err(|_| AttachmentError::InvalidEncoding)?;
        let lower = text.to_ascii_lowercase();

        let dangerous_keywords = [
            "<script",
            "</script",
            "javascript:",
            "onload=",
            "onerror=",
            "onclick=",
            "onmouseover=",
            "xlink:href=\"javascript:",
            "data:text/html",
        ];

        for kw in dangerous_keywords {
            if lower.contains(kw) {
                return Err(AttachmentError::DangerousSvgContent);
            }
        }

        Ok(())
    }

    pub fn detect_mime_type(data: &[u8]) -> Result<&'static str, AttachmentError> {
        if data.is_empty() {
            return Err(AttachmentError::FileEmpty);
        }

        if Self::is_executable_or_script(data) {
            return Err(AttachmentError::DisallowedExecutableOrScript);
        }

        if data.len() >= 8 && data.starts_with(b"\x89PNG\r\n\x1a\n") {
            return Ok("image/png");
        }

        if data.len() >= 3 && data.starts_with(&[0xFF, 0xD8, 0xFF]) {
            return Ok("image/jpeg");
        }

        if data.len() >= 6 && (data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a")) {
            return Ok("image/gif");
        }

        if data.len() >= 12 && data.starts_with(b"RIFF") && &data[8..12] == b"WEBP" {
            return Ok("image/webp");
        }

        if data.len() >= 5 && data.starts_with(b"%PDF-") {
            return Ok("application/pdf");
        }

        if data.len() >= 4 {
            let start = String::from_utf8_lossy(&data[..data.len().min(512)]).to_ascii_lowercase();
            if start.contains("<svg") {
                Self::scan_svg_safety(data)?;
                return Ok("image/svg+xml");
            }
        }

        if let Ok(text) = std::str::from_utf8(data) {
            let lower = text.trim_start().to_ascii_lowercase();
            if lower.starts_with("<!doctype html")
                || lower.starts_with("<html")
                || lower.starts_with("<head")
                || lower.starts_with("<body")
            {
                return Err(AttachmentError::UnsupportedMimeType("text/html is prohibited".into()));
            }

            let has_null_or_control = text
                .chars()
                .any(|c| (c as u32) < 0x20 && c != '\n' && c != '\r' && c != '\t');
            if !has_null_or_control {
                return Ok("text/plain");
            }
        }

        Err(AttachmentError::UnsupportedMimeType("Unknown or disallowed file format".into()))
    }

    pub fn extract_image_dimensions(mime_type: &str, data: &[u8]) -> Option<(u32, u32)> {
        match mime_type {
            "image/png" => {
                if data.len() >= 24 && &data[12..16] == b"IHDR" {
                    let w = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
                    let h = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
                    return Some((w, h));
                }
                None
            }
            "image/gif" => {
                if data.len() >= 10 {
                    let w = u16::from_le_bytes([data[6], data[7]]) as u32;
                    let h = u16::from_le_bytes([data[8], data[9]]) as u32;
                    return Some((w, h));
                }
                None
            }
            "image/webp" => {
                if data.len() >= 30 && &data[12..16] == b"VP8 " {
                    if data[23] == 0x9D && data[24] == 0x01 && data[25] == 0x2A {
                        let w = (u16::from_le_bytes([data[26], data[27]]) & 0x3FFF) as u32;
                        let h = (u16::from_le_bytes([data[28], data[29]]) & 0x3FFF) as u32;
                        return Some((w, h));
                    }
                }
                None
            }
            "image/jpeg" => {
                let mut idx = 2;
                while idx + 8 < data.len() {
                    if data[idx] != 0xFF {
                        break;
                    }
                    let marker = data[idx + 1];
                    let len = u16::from_be_bytes([data[idx + 2], data[idx + 3]]) as usize;
                    if marker == 0xC0 || marker == 0xC1 || marker == 0xC2 {
                        let h = u16::from_be_bytes([data[idx + 5], data[idx + 6]]) as u32;
                        let w = u16::from_be_bytes([data[idx + 7], data[idx + 8]]) as u32;
                        return Some((w, h));
                    }
                    idx += 2 + len;
                }
                None
            }
            _ => None,
        }
    }

    pub fn validate(
        uploader_pubkey: impl Into<String>,
        filename: &str,
        data: &[u8],
    ) -> Result<AttachmentMeta, AttachmentError> {
        let size_bytes = data.len();
        if size_bytes == 0 {
            return Err(AttachmentError::FileEmpty);
        }
        if size_bytes > MAX_ATTACHMENT_SIZE {
            return Err(AttachmentError::FileTooLarge {
                size: size_bytes,
                max: MAX_ATTACHMENT_SIZE,
            });
        }

        let mime_type = Self::detect_mime_type(data)?;
        let (width, height) = Self::extract_image_dimensions(mime_type, data)
            .map(|(w, h)| (Some(w), Some(h)))
            .unwrap_or((None, None));

        let sha256 = Self::compute_sha256(data);
        let sanitized_name = Self::sanitize_filename(filename);

        Ok(AttachmentMeta {
            id: uuid::Uuid::new_v4().to_string(),
            uploader_pubkey: uploader_pubkey.into(),
            filename: sanitized_name,
            mime_type: mime_type.to_string(),
            size_bytes,
            sha256,
            width,
            height,
            created_at: unix_now(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_png_validation_and_dimensions() {
        let mut png = vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A,
            0x00, 0x00, 0x00, 0x0D,
            0x49, 0x48, 0x44, 0x52,
            0x00, 0x00, 0x01, 0x00,
            0x00, 0x00, 0x00, 0x80,
            0x08, 0x06, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00,
        ];
        png.resize(64, 0);

        let meta = AttachmentValidator::validate("pubkey-1", "screenshot.png", &png).unwrap();
        assert_eq!(meta.mime_type, "image/png");
        assert_eq!(meta.width, Some(256));
        assert_eq!(meta.height, Some(128));
        assert_eq!(meta.size_bytes, 64);
        assert!(!meta.sha256.is_empty());
    }

    #[test]
    fn test_rejects_executables() {
        let exe = b"MZ\x90\x00\x03\x00\x00\x00";
        assert_eq!(
            AttachmentValidator::detect_mime_type(exe),
            Err(AttachmentError::DisallowedExecutableOrScript)
        );

        let elf = b"\x7fELF\x02\x01\x01\x00";
        assert_eq!(
            AttachmentValidator::detect_mime_type(elf),
            Err(AttachmentError::DisallowedExecutableOrScript)
        );

        let sh = b"#!/bin/bash\nrm -rf /";
        assert_eq!(
            AttachmentValidator::detect_mime_type(sh),
            Err(AttachmentError::DisallowedExecutableOrScript)
        );
    }

    #[test]
    fn test_rejects_malicious_svg() {
        let evil_svg = b"<svg><script>alert('xss')</script></svg>";
        assert_eq!(
            AttachmentValidator::detect_mime_type(evil_svg),
            Err(AttachmentError::DangerousSvgContent)
        );
    }

    #[test]
    fn test_filename_sanitization() {
        assert_eq!(
            AttachmentValidator::sanitize_filename("../../etc/passwd.png"),
            "passwd.png"
        );
        assert_eq!(
            AttachmentValidator::sanitize_filename("my cool photo (1).jpeg"),
            "my_cool_photo_1.jpeg"
        );
    }
}
