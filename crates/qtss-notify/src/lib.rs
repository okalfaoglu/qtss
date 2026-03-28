//! Çok kanallı bildirimler (Telegram, e-posta, webhook, …). `NotifyConfig::from_env` ile yapılandırılır.

pub mod config;
pub mod dispatch;
pub mod error;
pub mod types;

pub use config::NotifyConfig;
pub use dispatch::NotificationDispatcher;
pub use error::{NotifyError, NotifyResult};
pub use types::{DeliveryReceipt, Notification, NotificationChannel};
