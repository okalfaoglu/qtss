use qtss_storage::{
    AiApprovalRepository, AppConfigRepository, CopySubscriptionRepository,
    ExchangeAccountRepository, ExchangeFillRepository, ExchangeOrderRepository, NotifyOutboxRepository,
    PaperLedgerRepository, PnlRollupRepository, SystemConfigRepository, UserPermissionRepository,
    UserRepository,
};
use sqlx::PgPool;
use std::sync::Arc;

use crate::oauth::jwt::JwtIssuer;

pub struct AppState {
    pub pool: PgPool,
    pub http_client: reqwest::Client,
    pub setup_analysis_buffers: qtss_telegram_setup_analysis::SharedSetupBuffers,
    pub config: AppConfigRepository,
    pub pnl: PnlRollupRepository,
    pub exchange_accounts: ExchangeAccountRepository,
    pub exchange_orders: ExchangeOrderRepository,
    pub exchange_fills: ExchangeFillRepository,
    pub paper: PaperLedgerRepository,
    pub copy: CopySubscriptionRepository,
    pub ai_approval: AiApprovalRepository,
    pub notify_outbox: NotifyOutboxRepository,
    pub user_permissions: UserPermissionRepository,
    pub users: UserRepository,
    pub system_config: SystemConfigRepository,
    pub jwt: Option<JwtIssuer>,
    pub refresh_ttl_secs: i64,
}

impl AppState {
    pub async fn new(pool: PgPool) -> anyhow::Result<Self> {
        let system_config = SystemConfigRepository::new(pool.clone());

        let audience = qtss_storage::resolve_system_string(
            &pool,
            "api",
            "jwt_audience",
            "QTSS_JWT_AUD",
            "qtss-api",
        )
        .await;
        let issuer =
            qtss_storage::resolve_system_string(&pool, "api", "jwt_issuer", "QTSS_JWT_ISS", "qtss")
                .await;
        let access_ttl: i64 = qtss_storage::resolve_system_string(
            &pool,
            "api",
            "jwt_access_ttl_secs",
            "QTSS_ACCESS_TTL_SECS",
            "900",
        )
        .await
        .parse()
        .unwrap_or(900_i64);
        let refresh_ttl: i64 = qtss_storage::resolve_system_string(
            &pool,
            "api",
            "jwt_refresh_ttl_secs",
            "QTSS_REFRESH_TTL_SECS",
            "2592000",
        )
        .await
        .parse()
        .unwrap_or(2_592_000_i64);

        let jwt_secret = match system_config.get("api", "jwt_secret").await? {
            Some(row) => row
                .value
                .get("value")
                .and_then(|x| x.as_str())
                .unwrap_or_default()
                .trim()
                .to_string(),
            None => String::new(),
        };
        let jwt_secret = if !jwt_secret.is_empty() {
            jwt_secret
        } else {
            // No secret in DB (first run). Generate and persist; avoids env dependency.
            let mut bytes = [0u8; 48];
            rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut bytes);
            let generated = hex::encode(bytes);
            let _ = system_config
                .upsert(
                    "api",
                    "jwt_secret",
                    serde_json::json!({ "value": generated }),
                    Some(1),
                    Some("JWT HMAC secret (generated on first run)."),
                    Some(true),
                    None,
                )
                .await;
            // Use the plain generated value (the returned row is masked when is_secret=true).
            match system_config.get("api", "jwt_secret").await? {
                Some(row) => row
                    .value
                    .get("value")
                    .and_then(|x| x.as_str())
                    .unwrap_or_default()
                    .trim()
                    .to_string(),
                None => return Err(anyhow::anyhow!("failed to persist api.jwt_secret")),
            }
        };

        let jwt = JwtIssuer::from_secret(jwt_secret.as_bytes(), audience, issuer, access_ttl)
            .map_err(|e| anyhow::anyhow!(e))?;
        let config = AppConfigRepository::new(pool.clone());
        let pnl = PnlRollupRepository::new(pool.clone());
        let exchange_accounts = ExchangeAccountRepository::new(pool.clone());
        let exchange_orders = ExchangeOrderRepository::new(pool.clone());
        let exchange_fills = ExchangeFillRepository::new(pool.clone());
        let paper = PaperLedgerRepository::new(pool.clone());
        let copy = CopySubscriptionRepository::new(pool.clone());
        let ai_approval = AiApprovalRepository::new(pool.clone());
        let notify_outbox = NotifyOutboxRepository::new(pool.clone());
        let user_permissions = UserPermissionRepository::new(pool.clone());
        let users = UserRepository::new(pool.clone());
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .user_agent(concat!("qtss-api/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| anyhow::anyhow!("reqwest client: {e}"))?;
        Ok(Self {
            pool,
            http_client,
            setup_analysis_buffers: qtss_telegram_setup_analysis::SharedSetupBuffers::new(),
            config,
            pnl,
            exchange_accounts,
            exchange_orders,
            exchange_fills,
            paper,
            copy,
            ai_approval,
            notify_outbox,
            user_permissions,
            users,
            system_config,
            jwt: Some(jwt),
            refresh_ttl_secs: refresh_ttl,
        })
    }
}

pub type SharedState = Arc<AppState>;
