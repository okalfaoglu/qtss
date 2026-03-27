//! Ortak tipler, log seviyeleri ve uygulama modları (`live` / `dry`).

pub mod app_mode;
pub mod logging;

pub use app_mode::{AppMode, DbPersistenceMode};
pub use logging::{init_logging, log_business, log_critical, LogEvent, Loggable, QtssLogLevel};

/// Repo kökündeki `.env` dosyasını okur (`cargo run` çalışma dizini genelde kök). Yoksa sessizce geçer.
/// Aynı isimde ortam değişkeni zaten ayarlıysa `.env` içindeki değer onun yerine geçmez.
pub fn load_dotenv() {
    let _ = dotenvy::dotenv();
}
