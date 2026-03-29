//! Ortak tipler, log seviyeleri ve uygulama modları (`live` / `dry`).

pub mod app_mode;
pub mod database_url;
pub mod kill_switch;
pub mod logging;

pub use app_mode::{AppMode, DbPersistenceMode};
pub use database_url::{ensure_postgres_scheme, postgres_url_from_env_or_default};
pub use kill_switch::{clear_trading_halt, halt_trading, is_trading_halted, set_trading_halted};
pub use logging::{init_logging, log_business, log_critical, LogEvent, Loggable, QtssLogLevel};

/// Repo kökündeki `.env` dosyasını okur (`cargo run` çalışma dizini genelde kök). Yoksa sessizce geçer.
/// Aynı isimde ortam değişkeni zaten ayarlıysa `.env` içindeki değer onun yerine geçmez.
pub fn load_dotenv() {
    let _ = dotenvy::dotenv();
}
