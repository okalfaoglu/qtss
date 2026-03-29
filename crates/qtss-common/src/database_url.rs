//! `DATABASE_URL` okuma: tanımsız veya yalnız boşluk = varsayılan; şemasız URL’yi erken reddet.

/// Ortamda `DATABASE_URL` tanımlı ve trim sonrası boş değilse onu döner; aksi halde `default`.
pub fn postgres_url_from_env_or_default(default: &str) -> String {
    match std::env::var("DATABASE_URL") {
        Ok(s) => {
            let t = s.trim();
            if t.is_empty() {
                default.to_string()
            } else {
                t.to_string()
            }
        }
        Err(_) => default.to_string(),
    }
}

/// `postgres://` / `postgresql://` dışındaki değerler sqlx’te «relative URL without a base» üretir.
pub fn ensure_postgres_scheme(url: &str) -> Result<(), &'static str> {
    if url.starts_with("postgres://") || url.starts_with("postgresql://") {
        Ok(())
    } else {
        Err(
            "DATABASE_URL mutlak PostgreSQL adresi olmalı (postgres:// veya postgresql://). \
             Şemasız veya göreli değer kullanılamaz; .env içindeki satırı düzeltin.",
        )
    }
}
