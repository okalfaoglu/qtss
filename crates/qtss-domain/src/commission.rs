use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::exchange::ExchangeId;

/// Oran **kesir** olarak (ör. `0.001` = %0,1 = 10 bps notional üzerinden).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommissionQuote {
    pub maker_rate: Decimal,
    pub taker_rate: Decimal,
    pub source: CommissionSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommissionSource {
    ExchangeApi { exchange: ExchangeId },
    ConfigFallback { key: String },
}

pub trait CommissionResolver: Send + Sync {
    fn resolve(&self, exchange: ExchangeId, symbol: &str) -> CommissionQuote;
}

/// Komisyonun nereden geldiği: borsa API’si, sabit bps veya ikisinin birleşimi.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommissionPolicy {
    /// Yalnızca [`CommissionResolver`] / borsa uç noktası çıktısı.
    ExchangeApi,
    /// API yok veya VIP seviyesi simülasyonu — sabit **bps** (on binde bir).
    ManualBps {
        maker_bps: u32,
        taker_bps: u32,
    },
    /// API kotası varsa kullan; yoksa fallback bps.
    ExchangeApiWithFallback {
        fallback_maker_bps: u32,
        fallback_taker_bps: u32,
    },
}

impl Default for CommissionPolicy {
    fn default() -> Self {
        Self::ManualBps {
            maker_bps: 2,
            taker_bps: 5,
        }
    }
}

/// `bps` → notional oranı (ör. 5 bps → 0.0005).
#[must_use]
pub fn rate_from_bps(bps: u32) -> Decimal {
    Decimal::from(bps) / Decimal::from(10_000u32)
}

/// Notional × oran = ücret tutarı (quote cinsinden, basit model).
#[must_use]
pub fn commission_fee(notional: Decimal, rate: Decimal) -> Decimal {
    notional * rate
}

impl CommissionPolicy {
    /// Tek işlem için uygulanacak kesir oranı (`is_maker`: limit maker / post-only gibi).
    pub fn rate_for_fill(
        &self,
        is_maker: bool,
        from_exchange: Option<&CommissionQuote>,
    ) -> Result<Decimal, &'static str> {
        match self {
            Self::ExchangeApi => {
                let q = from_exchange.ok_or("borsa komisyon kotası yok (ExchangeApi)")?;
                Ok(if is_maker {
                    q.maker_rate
                } else {
                    q.taker_rate
                })
            }
            Self::ManualBps {
                maker_bps,
                taker_bps,
            } => {
                let bps = if is_maker { *maker_bps } else { *taker_bps };
                Ok(rate_from_bps(bps))
            }
            Self::ExchangeApiWithFallback {
                fallback_maker_bps,
                fallback_taker_bps,
            } => {
                if let Some(q) = from_exchange {
                    Ok(if is_maker {
                        q.maker_rate
                    } else {
                        q.taker_rate
                    })
                } else {
                    let bps = if is_maker {
                        *fallback_maker_bps
                    } else {
                        *fallback_taker_bps
                    };
                    Ok(rate_from_bps(bps))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_bps_to_rate() {
        let p = CommissionPolicy::ManualBps {
            maker_bps: 2,
            taker_bps: 5,
        };
        assert_eq!(p.rate_for_fill(true, None).unwrap(), Decimal::new(2, 4));
        assert_eq!(p.rate_for_fill(false, None).unwrap(), Decimal::new(5, 4));
    }

    #[test]
    fn exchange_api_requires_quote() {
        let p = CommissionPolicy::ExchangeApi;
        assert!(p.rate_for_fill(false, None).is_err());
    }
}
