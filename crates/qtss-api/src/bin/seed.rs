//! İlk kurulum: migrasyonlar (`migrations/*.sql`), varsayılan org, admin kullanıcı, OAuth istemcisi.
//! `DATABASE_URL=... QTSS_SEED_ADMIN_PASSWORD=... cargo run -p qtss-api --bin qtss-seed`

use anyhow::Context;
use qtss_common::load_dotenv;
use argon2::password_hash::{PasswordHasher, SaltString};
use argon2::Argon2;
use qtss_storage::{create_pool, run_migrations};
use uuid::Uuid;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    load_dotenv();
    let database_url =
        std::env::var("DATABASE_URL").context("DATABASE_URL gerekli")?;
    let admin_email =
        std::env::var("QTSS_SEED_ADMIN_EMAIL").unwrap_or_else(|_| "admin@localhost".into());
    let admin_password = std::env::var("QTSS_SEED_ADMIN_PASSWORD")
        .context("QTSS_SEED_ADMIN_PASSWORD gerekli")?;

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

    let client_secret: String = std::env::var("QTSS_SEED_CLIENT_SECRET")
        .unwrap_or_else(|_| Uuid::new_v4().to_string());
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

    println!("OK — org_id={org_id} user_id={uid}");
    println!("OAuth client_id={client_id}");
    println!("OAuth client_secret={client_secret}");
    Ok(())
}
