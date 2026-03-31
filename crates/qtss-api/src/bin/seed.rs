//! İlk kurulum: migrasyonlar (`migrations/*.sql`), varsayılan org, admin kullanıcı, OAuth istemcisi.
//! `DATABASE_URL=... QTSS_SEED_ADMIN_PASSWORD=... cargo run -p qtss-api --bin qtss-seed`

use anyhow::Context;
use qtss_common::{load_dotenv, require_postgres_database_url};
use argon2::password_hash::{PasswordHasher, SaltString};
use argon2::Argon2;
use qtss_storage::{create_pool, run_migrations, SystemConfigRepository};
use uuid::Uuid;

fn read_required_env_trimmed(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

async fn seed_exchange_account_if_present(
    pool: &sqlx::PgPool,
    user_id: Uuid,
    exchange: &str,
    segment: &str,
    api_key_env: &str,
    api_secret_env: &str,
) -> anyhow::Result<()> {
    let api_key = read_required_env_trimmed(api_key_env);
    let api_secret = read_required_env_trimmed(api_secret_env);
    let (Some(api_key), Some(api_secret)) = (api_key, api_secret) else {
        return Ok(());
    };
    sqlx::query(
        r#"INSERT INTO exchange_accounts (user_id, exchange, segment, api_key, api_secret)
           VALUES ($1, $2, $3, $4, $5)
           ON CONFLICT (user_id, exchange, segment)
           DO UPDATE SET api_key = EXCLUDED.api_key, api_secret = EXCLUDED.api_secret"#,
    )
    .bind(user_id)
    .bind(exchange)
    .bind(segment)
    .bind(api_key)
    .bind(api_secret)
    .execute(pool)
    .await?;
    Ok(())
}

fn json_string_value(v: &serde_json::Value) -> Option<String> {
    v.get("value")
        .and_then(|x| x.as_str())
        .or_else(|| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn generate_secret_hex(bytes_len: usize) -> String {
    let mut buf = vec![0u8; bytes_len];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut buf);
    hex::encode(buf)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    load_dotenv();
    let database_url = require_postgres_database_url().map_err(anyhow::Error::msg)?;

    let pool = create_pool(&database_url, 2).await.context("veritabanı bağlantısı")?;
    run_migrations(&pool).await.map_err(|e| {
        let msg = format!("{e:#}");
        if msg.contains("has been modified") {
            anyhow::anyhow!(
                "{msg}\n\nİpucu — geliştirme veritabanı: uygulanmış bir migrations/*.sql dosyası diskte değişti. Repo kökünden checksum’ları güncelleyin, sonra seed’i tekrarlayın:\n  cargo run -p qtss-storage --bin qtss-sync-sqlx-checksums\nKalıcı kalıp: eski migrasyonu ellemeden yeni numaralı .sql eklemek — docs/QTSS_CURSOR_DEV_GUIDE.md §6, docs/PROJECT.md."
            )
        } else {
            anyhow::Error::from(e)
        }
    }).context("SQL migrasyonları (tablo oluşturma)")?;

    let sys = SystemConfigRepository::new(pool.clone());
    let admin_email = match sys.get("seed", "admin_email").await? {
        Some(row) => json_string_value(&row.value).unwrap_or_else(|| "admin@localhost".into()),
        None => "admin@localhost".into(),
    };
    let admin_password = match sys.get("seed", "admin_password").await? {
        Some(row) => json_string_value(&row.value),
        None => None,
    }
    .or_else(|| std::env::var("QTSS_SEED_ADMIN_PASSWORD").ok())
    .unwrap_or_else(|| {
        let p = generate_secret_hex(24);
        p
    });
    if sys.get("seed", "admin_password").await?.is_none() {
        let _ = sys
            .upsert(
                "seed",
                "admin_password",
                serde_json::json!({ "value": admin_password }),
                Some(1),
                Some("Admin password (generated if missing)."),
                Some(true),
                None,
            )
            .await;
    }

    let org_id: Uuid =
        match sqlx::query_scalar::<_, Uuid>("SELECT id FROM organizations WHERE name = 'Default'")
            .fetch_optional(&pool)
            .await?
        {
            Some(id) => id,
            None => {
                sqlx::query_scalar("INSERT INTO organizations (name) VALUES ('Default') RETURNING id")
                    .fetch_one(&pool)
                    .await?
            }
        };

    let salt = SaltString::generate(&mut rand::thread_rng());
    let ph = Argon2::default()
        .hash_password(admin_password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!(e))?
        .to_string();

    let new_id = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO users (id, org_id, email, password_hash, display_name, is_admin)
           VALUES ($1, $2, $3, $4, 'Administrator', true)
           ON CONFLICT (email) DO NOTHING"#,
    )
    .bind(new_id)
    .bind(org_id)
    .bind(&admin_email)
    .bind(&ph)
    .execute(&pool)
    .await?;

    let uid: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE email = $1")
        .bind(&admin_email)
        .fetch_one(&pool)
        .await?;

    let admin_role: Uuid =
        sqlx::query_scalar("SELECT id FROM roles WHERE key = 'admin'")
            .fetch_one(&pool)
            .await?;
    sqlx::query(
        "INSERT INTO user_roles (user_id, role_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
    )
    .bind(uid)
    .bind(admin_role)
    .execute(&pool)
    .await?;

    let client_secret: String = match sys.get("seed", "oauth_client_secret").await? {
        Some(row) => json_string_value(&row.value).unwrap_or_else(|| generate_secret_hex(24)),
        None => std::env::var("QTSS_SEED_CLIENT_SECRET").ok().unwrap_or_else(|| generate_secret_hex(24)),
    };
    if sys.get("seed", "oauth_client_secret").await?.is_none() {
        let _ = sys
            .upsert(
                "seed",
                "oauth_client_secret",
                serde_json::json!({ "value": client_secret }),
                Some(1),
                Some("OAuth client_secret for qtss-cli (generated if missing)."),
                Some(true),
                None,
            )
            .await;
    }
    let salt_c = SaltString::generate(&mut rand::thread_rng());
    let client_ph = Argon2::default()
        .hash_password(client_secret.as_bytes(), &salt_c)
        .map_err(|e| anyhow::anyhow!(e))?
        .to_string();

    let client_id = "qtss-cli";
    let grants = vec![
        "password".to_string(),
        "refresh_token".to_string(),
        "client_credentials".to_string(),
    ];
    sqlx::query(
        r#"INSERT INTO oauth_clients (org_id, client_id, client_secret_hash, allowed_grant_types, service_user_id)
           VALUES ($1, $2, $3, $4, $5)
           ON CONFLICT (client_id) DO UPDATE SET
             client_secret_hash = EXCLUDED.client_secret_hash,
             allowed_grant_types = EXCLUDED.allowed_grant_types,
             service_user_id = EXCLUDED.service_user_id"#,
    )
    .bind(org_id)
    .bind(client_id)
    .bind(&client_ph)
    .bind(&grants)
    .bind(uid)
    .execute(&pool)
    .await?;

    // Optional: seed Binance credentials into exchange_accounts for the admin user.
    seed_exchange_account_if_present(
        &pool,
        uid,
        "binance",
        "spot",
        "QTSS_BINANCE_SPOT_API_KEY",
        "QTSS_BINANCE_SPOT_API_SECRET",
    )
    .await?;
    seed_exchange_account_if_present(
        &pool,
        uid,
        "binance",
        "futures",
        "QTSS_BINANCE_FUTURES_API_KEY",
        "QTSS_BINANCE_FUTURES_API_SECRET",
    )
    .await?;

    println!("OK — org_id={org_id} user_id={uid}");
    println!("OAuth client_id={client_id}");
    println!("OAuth client_secret={client_secret}");
    Ok(())
}
