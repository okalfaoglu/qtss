use qtss_storage::{
    AiApprovalRepository, AppConfigRepository, CopySubscriptionRepository,
    ExchangeAccountRepository, ExchangeOrderRepository, NotifyOutboxRepository,
    PaperLedgerRepository, PnlRollupRepository,
};
use sqlx::PgPool;
use std::sync::Arc;

use crate::oauth::jwt::JwtIssuer;

pub struct AppState {
    pub pool: PgPool,
    pub config: AppConfigRepository,
    pub pnl: PnlRollupRepository,
    pub exchange_accounts: ExchangeAccountRepository,
    pub exchange_orders: ExchangeOrderRepository,
    pub paper: PaperLedgerRepository,
    pub copy: CopySubscriptionRepository,
    pub ai_approval: AiApprovalRepository,
    pub notify_outbox: NotifyOutboxRepository,
    pub jwt: Option<JwtIssuer>,
    pub refresh_ttl_secs: i64,
}

impl AppState {
    pub fn new(pool: PgPool) -> anyhow::Result<Self> {
        let jwt_secret = std::env::var("QTSS_JWT_SECRET")
            .map_err(|_| anyhow::anyhow!("QTSS_JWT_SECRET ortam değişkeni gerekli"))?;
        let audience = std::env::var("QTSS_JWT_AUD").unwrap_or_else(|_| "qtss-api".into());
        let issuer = std::env::var("QTSS_JWT_ISS").unwrap_or_else(|_| "qtss".into());
        let access_ttl = std::env::var("QTSS_ACCESS_TTL_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(900_i64);
        let refresh_ttl = std::env::var("QTSS_REFRESH_TTL_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(2_592_000_i64);

        let jwt = JwtIssuer::from_secret(jwt_secret.as_bytes(), audience, issuer, access_ttl)
            .map_err(|e| anyhow::anyhow!(e))?;
        let config = AppConfigRepository::new(pool.clone());
        let pnl = PnlRollupRepository::new(pool.clone());
        let exchange_accounts = ExchangeAccountRepository::new(pool.clone());
        let exchange_orders = ExchangeOrderRepository::new(pool.clone());
        let paper = PaperLedgerRepository::new(pool.clone());
        let copy = CopySubscriptionRepository::new(pool.clone());
        let ai_approval = AiApprovalRepository::new(pool.clone());
        let notify_outbox = NotifyOutboxRepository::new(pool.clone());
        Ok(Self {
            pool,
            config,
            pnl,
            exchange_accounts,
            exchange_orders,
            paper,
            copy,
            ai_approval,
            notify_outbox,
            jwt: Some(jwt),
            refresh_ttl_secs: refresh_ttl,
        })
    }
}

pub type SharedState = Arc<AppState>;
