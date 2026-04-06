//! Çok kanallı bildirimler (Telegram, e-posta, webhook, …). `NotifyConfig::from_env` ile yapılandırılır.

pub mod config;
pub mod dispatch;
pub mod error;
pub mod locale;
pub mod telegram_html;
pub mod types;

pub use config::NotifyConfig;
pub use dispatch::NotificationDispatcher;
pub use error::{NotifyError, NotifyResult};
pub use locale::resolve_bilingual;
pub use telegram_html::escape_telegram_html;
pub use types::{DeliveryReceipt, Notification, NotificationChannel};
