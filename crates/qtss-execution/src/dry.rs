//! Canlı veri + **sanal yürütme** — borsaya emir gitmez; [`VirtualLedgerParams`] ile nakit / taban stok takibi.
//!
//! Kalıcı defter ve dolum kayıtları uygulama katmanında (`paper_balances` / `paper_fills`).

use std::collections::HashMap;
use std::sync::RwLock;

use async_trait::async_trait;
use qtss_common::{log_business, Loggable, QtssLogLevel};
use qtss_domain::commission::{commission_fee, CommissionPolicy, CommissionQuote};
use qtss_domain::execution::VirtualLedgerParams;
use qtss_domain::exchange::{ExchangeId, MarketSegment};
use qtss_domain::orders::{OrderIntent, OrderSide, OrderType};
use rust_decimal::Decimal;
use tracing::instrument;
use uuid::Uuid;

use crate::gateway::{ExecutionError, ExecutionGateway, FillEvent};

/// Enstrüman bazlı taban pozisyon anahtarı: `binance:spot:BTCUSDT`.
#[must_use]
pub fn instrument_position_key(i: &qtss_domain::symbol::InstrumentId) -> String {
    let ex = match i.exchange {
        ExchangeId::Binance => "binance",
        ExchangeId::Custom => "custom",
    };
    let seg = match i.segment {
        MarketSegment::Spot => "spot",
        MarketSegment::Futures => "futures",
        MarketSegment::Margin => "margin",
        MarketSegment::Options => "options",
    };
    format!("{ex}:{seg}:{}", i.symbol.trim().to_ascii_uppercase())
}

/// Dry-run defter durumu — bellekte veya DB satırından yüklenir.
#[derive(Debug, Clone)]
pub struct DryLedgerState {
    pub quote_balance: Decimal,
    pub base_by_symbol: HashMap<String, Decimal>,
    /// Market emirleri için referans fiyat (son işlem / işaret). Anahtar: [`instrument_position_key`].
    pub marks: HashMap<String, Decimal>,
}

impl DryLedgerState {
    #[must_use]
    pub fn from_params(p: VirtualLedgerParams) -> Self {
        Self {
            quote_balance: p.initial_quote_balance,
            base_by_symbol: HashMap::new(),
            marks: HashMap::new(),
        }
    }

    fn base_qty(&self, key: &str) -> Decimal {
        self.base_by_symbol.get(key).copied().unwrap_or(Decimal::ZERO)
    }
}

/// Sanal emir sonucu — DB `paper_fills` ve API yanıtı için.
#[derive(Debug, Clone)]
pub struct DryPlaceOutcome {
    pub client_order_id: Uuid,
    pub fill: FillEvent,
    pub quote_balance_after: Decimal,
    pub base_positions_after: HashMap<String, Decimal>,
}

fn fill_price(intent: &OrderIntent, mark: Decimal) -> Result<Decimal, ExecutionError> {
    match &intent.order_type {
        OrderType::Market => Ok(mark),
        OrderType::Limit { price, .. } => Ok(*price),
        // Bracket / reduce-only simülasyonu: tetik fiyatı ile anında dolum (paper).
        OrderType::StopMarket { stop_price }
        | OrderType::TakeProfitMarket { stop_price } => Ok(*stop_price),
        _ => Err(ExecutionError::Other(
            "dry: bu emir tipi paper’da simüle edilmez (Market/Limit/StopMarket/TakeProfitMarket)".into(),
        )),
    }
}

fn is_maker_fill(intent: &OrderIntent) -> bool {
    matches!(
        &intent.order_type,
        OrderType::Limit {
            post_only: true,
            ..
        }
    )
}

/// Tek bir emir için defteri günceller (idempotent değildir — çağıran transaction içinde kullanmalı).
pub fn apply_place(
    ledger: &mut DryLedgerState,
    policy: &CommissionPolicy,
    commission_quote: Option<&CommissionQuote>,
    intent: OrderIntent,
    mark_override: Option<Decimal>,
) -> Result<DryPlaceOutcome, ExecutionError> {
    if intent.requires_human_approval {
        return Err(ExecutionError::PendingApproval);
    }
    if intent.quantity <= Decimal::ZERO {
        return Err(ExecutionError::Other("dry: miktar pozitif olmalı".into()));
    }

    let key = instrument_position_key(&intent.instrument);
    let mark_from_map = ledger.marks.get(&key).copied();
    let mark = mark_override
        .or(mark_from_map)
        .ok_or_else(|| ExecutionError::Other(format!("dry: referans fiyat yok ({key})")))?;

    let price = fill_price(&intent, mark)?;
    if price <= Decimal::ZERO {
        return Err(ExecutionError::Other("dry: işlem fiyatı pozitif olmalı".into()));
    }

    let is_maker = is_maker_fill(&intent);
    let rate = policy
        .rate_for_fill(is_maker, commission_quote)
        .map_err(|m| ExecutionError::Other(m.into()))?;
    let notional = intent.quantity * price;
    let fee = commission_fee(notional, rate);
    let cid = Uuid::new_v4();

    match intent.side {
        OrderSide::Buy => {
            let total = notional + fee;
            if ledger.quote_balance < total {
                return Err(ExecutionError::InsufficientPaper);
            }
            ledger.quote_balance -= total;
            *ledger.base_by_symbol.entry(key.clone()).or_insert(Decimal::ZERO) += intent.quantity;
        }
        OrderSide::Sell => {
            let proceeds = notional - fee;
            if proceeds < Decimal::ZERO {
                return Err(ExecutionError::Other("dry: komisyon brüt tutarı aştı".into()));
            }
            match intent.instrument.segment {
                MarketSegment::Futures => {
                    let new_base = ledger.base_qty(&key) - intent.quantity;
                    ledger.quote_balance += proceeds;
                    if new_base.is_zero() {
                        ledger.base_by_symbol.remove(&key);
                    } else {
                        ledger.base_by_symbol.insert(key.clone(), new_base);
                    }
                }
                _ => {
                    let base = ledger.base_qty(&key);
                    if base < intent.quantity {
                        return Err(ExecutionError::InsufficientPaper);
                    }
                    *ledger.base_by_symbol.entry(key.clone()).or_insert(Decimal::ZERO) -= intent.quantity;
                    if ledger.base_by_symbol[&key].is_zero() {
                        ledger.base_by_symbol.remove(&key);
                    }
                    ledger.quote_balance += proceeds;
                }
            }
        }
    }

    let fill = FillEvent {
        client_order_id: cid,
        avg_price: price,
        quantity: intent.quantity,
        fee,
    };

    Ok(DryPlaceOutcome {
        client_order_id: cid,
        fill,
        quote_balance_after: ledger.quote_balance,
        base_positions_after: ledger.base_by_symbol.clone(),
    })
}

pub struct DryRunGateway {
    ledger: RwLock<DryLedgerState>,
    policy: CommissionPolicy,
    commission_quote: Option<CommissionQuote>,
}

impl Loggable for DryRunGateway {
    const MODULE: &'static str = "qtss_execution::dry";
}

impl DryRunGateway {
    #[must_use]
    pub fn new(params: VirtualLedgerParams, policy: CommissionPolicy, cq: Option<CommissionQuote>) -> Self {
        Self {
            ledger: RwLock::new(DryLedgerState::from_params(params)),
            policy,
            commission_quote: cq,
        }
    }

    #[must_use]
    pub fn from_ledger(
        ledger: DryLedgerState,
        policy: CommissionPolicy,
        cq: Option<CommissionQuote>,
    ) -> Self {
        Self {
            ledger: RwLock::new(ledger),
            policy,
            commission_quote: cq,
        }
    }

    /// Market emri öncesi son işlem fiyatını kaydet (worker / strateji hattı).
    pub fn set_mark(&self, instrument: &qtss_domain::symbol::InstrumentId, price: Decimal) -> Result<(), ExecutionError> {
        if price <= Decimal::ZERO {
            return Err(ExecutionError::Other("dry: mark fiyatı pozitif olmalı".into()));
        }
        let key = instrument_position_key(instrument);
        self.ledger
            .write()
            .map_err(|_| ExecutionError::Other("dry: defter kilit hatası".into()))?
            .marks
            .insert(key, price);
        Ok(())
    }

    pub fn place_detailed(
        &self,
        intent: OrderIntent,
        mark_override: Option<Decimal>,
    ) -> Result<DryPlaceOutcome, ExecutionError> {
        let mut lg = self
            .ledger
            .write()
            .map_err(|_| ExecutionError::Other("dry: defter kilit hatası".into()))?;
        apply_place(
            &mut lg,
            &self.policy,
            self.commission_quote.as_ref(),
            intent,
            mark_override,
        )
    }

    pub fn snapshot_ledger(&self) -> Result<DryLedgerState, ExecutionError> {
        self.ledger
            .read()
            .map_err(|_| ExecutionError::Other("dry: defter kilit hatası".into()))
            .map(|g| g.clone())
    }
}

#[async_trait]
impl ExecutionGateway for DryRunGateway {
    fn set_reference_price(
        &self,
        instrument: &qtss_domain::symbol::InstrumentId,
        price: Decimal,
    ) -> Result<(), ExecutionError> {
        self.set_mark(instrument, price)
    }

    #[instrument(skip(self, intent))]
    async fn place(&self, intent: OrderIntent) -> Result<Uuid, ExecutionError> {
        let out = self.place_detailed(intent, None)?;
        log_business(
            QtssLogLevel::Info,
            Self::MODULE,
            format!(
                "dry place cid={} qty {} price {}",
                out.client_order_id, out.fill.quantity, out.fill.avg_price
            ),
        );
        Ok(out.client_order_id)
    }

    async fn cancel(&self, _client_order_id: Uuid) -> Result<(), ExecutionError> {
        log_business(QtssLogLevel::Debug, Self::MODULE, "dry cancel (no-op — anında dolum varsayımı)");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qtss_domain::symbol::InstrumentId;

    fn btcusdt_spot() -> InstrumentId {
        InstrumentId {
            exchange: ExchangeId::Binance,
            segment: MarketSegment::Spot,
            symbol: "BTCUSDT".into(),
        }
    }

    #[test]
    fn buy_reduces_quote_increases_base() {
        let mut ledger = DryLedgerState::from_params(VirtualLedgerParams {
            initial_quote_balance: Decimal::new(50_000, 0),
        });
        let intent = OrderIntent {
            instrument: btcusdt_spot(),
            side: OrderSide::Buy,
            quantity: Decimal::ONE,
            order_type: OrderType::Market,
            time_in_force: qtss_domain::orders::TimeInForce::Gtc,
            requires_human_approval: false,
            futures: None,
        };
        let out = apply_place(
            &mut ledger,
            &CommissionPolicy::default(),
            None,
            intent,
            Some(Decimal::new(40_000, 0)),
        )
        .unwrap();
        assert_eq!(out.fill.avg_price, Decimal::new(40_000, 0));
        assert!(out.quote_balance_after < Decimal::new(50_000, 0));
        let k = instrument_position_key(&btcusdt_spot());
        assert_eq!(ledger.base_qty(&k), Decimal::ONE);
    }

    #[test]
    fn sell_requires_base() {
        let mut ledger = DryLedgerState::from_params(VirtualLedgerParams {
            initial_quote_balance: Decimal::new(50_000, 0),
        });
        let k = instrument_position_key(&btcusdt_spot());
        ledger.base_by_symbol.insert(k.clone(), Decimal::ONE);
        let intent = OrderIntent {
            instrument: btcusdt_spot(),
            side: OrderSide::Sell,
            quantity: Decimal::ONE,
            order_type: OrderType::Market,
            time_in_force: qtss_domain::orders::TimeInForce::Gtc,
            requires_human_approval: false,
            futures: None,
        };
        let out = apply_place(
            &mut ledger,
            &CommissionPolicy::default(),
            None,
            intent,
            Some(Decimal::new(40_000, 0)),
        )
        .unwrap();
        assert_eq!(out.base_positions_after.get(&k).copied().unwrap_or(Decimal::ZERO), Decimal::ZERO);
        assert!(out.quote_balance_after > Decimal::new(50_000, 0));
    }

    #[test]
    fn insufficient_quote_on_buy() {
        let mut ledger = DryLedgerState::from_params(VirtualLedgerParams {
            initial_quote_balance: Decimal::ONE,
        });
        let intent = OrderIntent {
            instrument: btcusdt_spot(),
            side: OrderSide::Buy,
            quantity: Decimal::new(10, 0),
            order_type: OrderType::Market,
            time_in_force: qtss_domain::orders::TimeInForce::Gtc,
            requires_human_approval: false,
            futures: None,
        };
        let err = apply_place(
            &mut ledger,
            &CommissionPolicy::default(),
            None,
            intent,
            Some(Decimal::new(40_000, 0)),
        )
        .unwrap_err();
        assert!(matches!(err, ExecutionError::InsufficientPaper));
    }

    fn btcusdt_futures() -> InstrumentId {
        InstrumentId {
            exchange: ExchangeId::Binance,
            segment: MarketSegment::Futures,
            symbol: "BTCUSDT".into(),
        }
    }

    #[test]
    fn futures_sell_opens_short_without_prior_base() {
        let mut ledger = DryLedgerState::from_params(VirtualLedgerParams {
            initial_quote_balance: Decimal::new(50_000, 0),
        });
        let intent = OrderIntent {
            instrument: btcusdt_futures(),
            side: OrderSide::Sell,
            quantity: Decimal::new(1, 1),
            order_type: OrderType::Market,
            time_in_force: qtss_domain::orders::TimeInForce::Gtc,
            requires_human_approval: false,
            futures: None,
        };
        let out = apply_place(
            &mut ledger,
            &CommissionPolicy::default(),
            None,
            intent,
            Some(Decimal::new(40_000, 0)),
        )
        .unwrap();
        let k = instrument_position_key(&btcusdt_futures());
        assert_eq!(
            out.base_positions_after.get(&k).copied().unwrap_or(Decimal::ZERO),
            Decimal::new(-1, 1)
        );
        assert!(out.quote_balance_after > Decimal::new(50_000, 0));
    }
}
