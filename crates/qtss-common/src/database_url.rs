//! `DATABASE_URL` okuma: trim, BOM/tırnak temizliği; ortam şemasız/hatalıysa kök `.env` yedeği.
//!
//! `dotenvy::dotenv()` ortamda değişken zaten varsa `.env` ile **üzerine yazmaz**; bu yüzden shell’de
//! kalmış şemasız bir `DATABASE_URL` doğru `.env` satırını gölgeleyebilir — burada yedek okuma uygulanır.

use std::fs;
use std::path::Path;

/// Ortam + gerekirse `.env` dosyası; hiçbiri geçerli değilse `default`.
pub fn postgres_url_from_env_or_default(default: &str) -> String {
    let env_candidate = std::env::var("DATABASE_URL")
        .ok()
        .map(|s| normalize_database_url(&s))
        .filter(|s| !s.is_empty());

    if let Some(ref n) = env_candidate {
        if postgres_scheme_ok(n) {
            return n.clone();
        }
        if let Some(v) = read_database_url_from_dotenv_file(Path::new(".env")) {
            let nf = normalize_database_url(&v);
            if !nf.is_empty() && postgres_scheme_ok(&nf) {
                return nf;
            }
        }
        return n.clone();
    }

    if let Some(v) = read_database_url_from_dotenv_file(Path::new(".env")) {
        let nf = normalize_database_url(&v);
        if !nf.is_empty() && postgres_scheme_ok(&nf) {
            return nf;
        }
    }

    default.to_string()
}

/// CLI (sync/seed): geçerli URL yoksa hata metni.
pub fn require_postgres_database_url() -> Result<String, String> {
    let u = postgres_url_from_env_or_default("");
    if u.trim().is_empty() {
        return Err(
            "DATABASE_URL gerekli (.env veya ortam). Boş bırakmayın; örnek: postgres://user:pass@127.0.0.1:5432/qtss"
                .into(),
        );
    }
    ensure_postgres_scheme(&u)?;
    Ok(u)
}

fn postgres_scheme_ok(s: &str) -> bool {
    s.starts_with("postgres://") || s.starts_with("postgresql://")
}

fn normalize_database_url(s: &str) -> String {
    let mut t = s.trim().to_string();
    if t.starts_with('\u{feff}') {
        t = t.trim_start_matches('\u{feff}').to_string();
    }
    t = t.trim().to_string();
    if t.len() >= 2 {
        let bytes = t.as_bytes();
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            t = t[1..t.len() - 1].trim().to_string();
        }
    }
    t
}

fn read_database_url_from_dotenv_file(path: &Path) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line).trim();
        let (key, value) = line.split_once('=')?;
        if key.trim() != "DATABASE_URL" {
            continue;
        }
        return Some(value.to_string());
    }
    None
}

/// `postgres://` / `postgresql://` dışındaki değerler sqlx’te «relative URL without a base» üretir.
pub fn ensure_postgres_scheme(url: &str) -> Result<(), &'static str> {
    let n = normalize_database_url(url);
    if postgres_scheme_ok(&n) {
        Ok(())
    } else {
        Err(
            "DATABASE_URL mutlak PostgreSQL adresi olmalı (postgres:// veya postgresql://). \
             Shell’de şemasız veya yanlış bir değer kalmış olabilir: `echo \"$DATABASE_URL\"` — \
             gerekirse `unset DATABASE_URL` yapıp yalnız kök `.env` kullanın. \
             Değer tırnaklı veya BOM’lu ise normalize edin.",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_bom_and_quotes() {
        let s = format!("\u{feff}\"postgres://a@b/c\"");
        assert_eq!(normalize_database_url(&s), "postgres://a@b/c");
    }
}
