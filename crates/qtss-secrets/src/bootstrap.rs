//! KEK bootstrap helpers — reads the active master key out of an env
//! variable and materialises a `StaticKek`. Production deployments
//! should swap this for a KMS-backed `KekProvider`; single-node boxes
//! run the static version.
//!
//! Env variable layout:
//!   * `QTSS_SECRET_KEK_V1` → 64-hex-char string (32 raw bytes, AES-256).
//!   * `QTSS_SECRET_KEK_VERSION` → integer version (defaults to 1);
//!     must line up with a `QTSS_SECRET_KEK_V<N>` entry.
//!
//! The loader is strict — missing or malformed KEK material is a hard
//! error so a misconfigured node fails loud on startup rather than
//! silently serving plaintext from the fallback path.

use crate::kek::StaticKek;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum KekBootstrapError {
    #[error("env variable {0} not set")]
    Missing(String),
    #[error("env variable {0} is not valid hex: {1}")]
    BadHex(String, String),
    #[error("kek material must be exactly 32 bytes — got {0}")]
    BadLength(usize),
    #[error("invalid kek version: {0}")]
    BadVersion(String),
}

pub fn load_static_kek_from_env() -> Result<StaticKek, KekBootstrapError> {
    let version = std::env::var("QTSS_SECRET_KEK_VERSION")
        .unwrap_or_else(|_| "1".to_string())
        .parse::<i32>()
        .map_err(|e| KekBootstrapError::BadVersion(e.to_string()))?;
    let var_name = format!("QTSS_SECRET_KEK_V{version}");
    let hex = std::env::var(&var_name)
        .map_err(|_| KekBootstrapError::Missing(var_name.clone()))?;
    let raw = parse_hex(&hex)
        .map_err(|e| KekBootstrapError::BadHex(var_name.clone(), e))?;
    if raw.len() != 32 {
        return Err(KekBootstrapError::BadLength(raw.len()));
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&raw);
    Ok(StaticKek::new(version, key))
}

fn parse_hex(s: &str) -> Result<Vec<u8>, String> {
    let s = s.trim();
    if s.len() % 2 != 0 {
        return Err("odd-length hex string".to_string());
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    for i in (0..s.len()).step_by(2) {
        let byte_str = &s[i..i + 2];
        let byte = u8::from_str_radix(byte_str, 16)
            .map_err(|e| format!("bad hex pair '{byte_str}': {e}"))?;
        out.push(byte);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_roundtrip() {
        let raw = parse_hex("00ff10").unwrap();
        assert_eq!(raw, vec![0x00, 0xff, 0x10]);
    }

    #[test]
    fn odd_length_rejected() {
        assert!(parse_hex("abc").is_err());
    }
}
