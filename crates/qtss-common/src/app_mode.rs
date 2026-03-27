use serde::{Deserialize, Serialize};

/// Çalışma modu: canlı işlem veya sanal kasa (dry) ile aynı canlı veri akışı.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppMode {
    /// Canlı veri, gerçek emir, gerçek sonuçların kalıcı yazımı.
    Live,
    /// Canlı veri, sanal bakiye/emir; sonuçlar DB’de ayrı namespace/tablo ile izlenir.
    Dry,
}

impl AppMode {
    pub fn from_config(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "live" => Some(Self::Live),
            "dry" => Some(Self::Dry),
            _ => None,
        }
    }
}

/// DB’ye yazarken hangi “iz” altında yazılacağı (dry run kayıtlarını ayırmak için).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbPersistenceMode {
    LiveLedger,
    DryLedger,
}

impl From<AppMode> for DbPersistenceMode {
    fn from(value: AppMode) -> Self {
        match value {
            AppMode::Live => Self::LiveLedger,
            AppMode::Dry => Self::DryLedger,
        }
    }
}
