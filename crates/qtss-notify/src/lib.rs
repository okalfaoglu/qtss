//! Çok kanallı bildirimler (Telegram, e-posta, webhook, …). `NotifyConfig::from_env` ile yapılandırılır.

pub mod card;
pub mod config;
pub mod digest_render;
pub mod dispatch;
pub mod error;
pub mod health;
pub mod lifecycle;
pub mod lifecycle_handlers;
pub mod locale;
pub mod poz_koruma;
pub mod price_tick;
pub mod smart_target;
pub mod telegram_html;
pub mod telegram_render;
pub mod types;
pub mod x_render;

pub use card::{
    AssetCategory, CategoryThresholds, PublicCard, ScoreTier, SetupDirection, SetupSnapshot,
    TargetPoint, TierBadge, TierThresholds,
};
pub use config::NotifyConfig;
pub use health::{
    compute as compute_health, load_health_bands, load_health_weights,
    HealthBand, HealthBands, HealthComponents, HealthScore, HealthWeights,
};
pub use lifecycle::{
    detect_transitions, make_context, promote_tp_hit, LifecycleContext, LifecycleDecision,
    LifecycleEventKind, LifecycleHandler, LifecycleRouter, WatcherSetupState,
};
pub use lifecycle_handlers::{DbPersistHandler, TelegramLifecycleHandler, XOutboxHandler};
pub use digest_render::{default_window as digest_default_window, render_digest, DigestRenderInput};
pub use telegram_render::{render_lifecycle, render_public_card};
pub use x_render::{render_lifecycle_x, render_public_card_x, X_MAX_CHARS};
pub use poz_koruma::{
    evaluate as evaluate_poz_koruma, load_config as load_poz_koruma_config,
    PozKorumaConfig, RatchetInput, RatchetOutcome, RatchetStep,
};
pub use price_tick::{PriceKey, PriceTick, PriceTickStore};
pub use smart_target::{
    decide as decide_smart_target, load_config as load_smart_target_config, rule_evaluate,
    DefaultLlmJudge, LlmJudge, SmartTargetAction, SmartTargetCfg, SmartTargetDecision,
    SmartTargetEvaluatorKind, SmartTargetInput,
};
pub use dispatch::NotificationDispatcher;
pub use error::{NotifyError, NotifyResult};
pub use locale::resolve_bilingual;
pub use telegram_html::escape_telegram_html;
pub use types::{DeliveryReceipt, Notification, NotificationChannel};
