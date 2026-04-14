# Faz 8.0c — Live Execution Pipeline

**Status:** Planlandı. Faz 8.0a (Wyckoff Setup Engine — dry+backtest) tamamlandıktan sonra başlatılacak.

**Önkoşul:** En az 4 hafta paper trading verisi toplanmış, Wyckoff setup performansı doğrulanmış olmalı.

---

## 1. Mevcut Kod Envanteri

| Katman | Dosya | Durum |
|---|---|---|
| Binance HMAC sign | `crates/qtss-binance/src/sign.rs` | ✅ Var |
| Binance REST core | `crates/qtss-binance/src/rest.rs` | ✅ Var |
| Spot order endpoints | `crates/qtss-binance/src/spot.rs` | ✅ Var |
| Futures `POST /fapi/v1/order` | `crates/qtss-binance/src/futures.rs:281` (`fapi_new_order`) | ✅ Var |
| Futures cancel/batch | `futures.rs:339, 351` | ✅ Var |
| Order ID parse | `crates/qtss-binance/src/order_parse.rs` | ✅ Var |
| Commission tablosu | `crates/qtss-binance/src/commission.rs` | ✅ Var |
| Q-RADAR sanal portföy | `crates/qtss-storage/src/q_radar_portfolio.rs` | ✅ Dry-only |
| **ExecutionAdapter trait** | — | ❌ Yok |
| **Mode dispatcher** | — | ❌ Yok |
| **Reconciliation loop** | — | ❌ Yok |
| **Risk circuit breaker** | — | ❌ Yok |
| **Order state machine** | — | ❌ Yok |
| **Audit/order tablosu** | — | ❌ Yok |

---

## 2. Adımlar

### 2.1 Migration `0064_live_execution.sql`
```sql
qtss_v2_orders          -- her broker order'ı (state machine)
  id, setup_id FK, mode, venue, symbol, side, type, qty, price,
  venue_order_id, client_order_id, state, fills JSONB, error,
  submitted_at, ack_at, last_update_at
  state ∈ pending|submitted|acknowledged|partially_filled|filled|cancelled|rejected|error

qtss_v2_positions       -- canlı pozisyon snapshot (broker reconciled)
  id, setup_id FK, symbol, side, qty, avg_entry, mark_price,
  unrealized_pnl, last_synced_at

qtss_v2_risk_circuit    -- circuit breaker state
  id, scope (global|venue|symbol), key, state (open|tripped),
  trip_reason, tripped_at, reset_at
```
Config seed: `setup.live.*` (api key references, risk limits, recon interval, allowed venues).

### 2.2 `qtss-execution` yeni crate
```rust
trait ExecutionAdapter: Send + Sync {
    async fn submit(&self, OrderRequest) -> Result<OrderAck>;
    async fn cancel(&self, venue_order_id: &str) -> Result<()>;
    async fn fetch_position(&self, symbol: &str) -> Result<Option<Position>>;
    async fn fetch_balance(&self) -> Result<Balance>;
    async fn set_leverage(&self, symbol: &str, leverage: u8) -> Result<()>;
    async fn set_position_mode(&self, hedge: bool) -> Result<()>;
    fn venue_class(&self) -> VenueClass;
}

// Implementations
struct BinanceFuturesAdapter { client: BinanceClient }
struct DryRunAdapter { pool: PgPool }      // virtual fills (current Q-RADAR)
struct BacktestAdapter { history_iter: Box<dyn BarFeed> }
```

### 2.3 Dispatcher
```rust
fn dispatch(mode: Mode, venue: &str) -> Box<dyn ExecutionAdapter> {
    // Lookup table — CLAUDE.md #1 (no central match arm)
    static REGISTRY: HashMap<(Mode, &str), AdapterFactory> = ...
}
```

### 2.4 Order State Machine
- Transitions: `pending → submitted → acknowledged → partially_filled | filled | cancelled | rejected | error`
- Idempotent (`client_order_id` deterministic from setup_id + attempt #)
- Retry policy with exponential backoff for transient errors (5xx, network)

### 2.5 `qtss-worker/src/reconciliation_loop.rs`
- Her 1s: aktif live setup'ları gez → broker'dan position çek → DB ile karşılaştır
- Drift varsa: alarm + circuit_open
- Fill event'lerini consume → setup state'i güncelle (TP1 hit, SL hit)

### 2.6 `qtss-worker/src/risk_circuit_breaker.rs`
- **Daily loss limit**: günlük PnL < `setup.live.daily_max_loss_pct` → tüm live setup'lar durdurulur, sadece kapatma izni
- **Symbol max loss**: tek sembolde günlük loss > `symbol_max_loss_pct` → o sembol kilitlenir
- **API rate guard**: 429 alındıysa 60s exponential backoff
- **Position drift**: DB ↔ broker drift > `drift_tolerance_pct` → instant close + halt
- **Heartbeat loss**: WS bağlantısı koptu + 30s → tüm pending order'lar cancel
- Trip → `qtss_v2_risk_circuit` row + structured log + frontend banner

### 2.7 Setup Engine entegrasyonu
`crates/qtss-worker/src/v2_setup_loop.rs`:
```rust
match setup.mode {
    Mode::Dry      => virtual_fill(setup),                       // mevcut
    Mode::Live     => {
        circuit_breaker.check(&setup)?;                          // gate
        let adapter = dispatcher::for_setup(&setup);
        let req = OrderRequest::from_plan(&plan);
        let ack = adapter.submit(req).await?;
        order_state.transition(ack)?;
        // OCO entries: SL stop-market + TP limit ladder
    }
    Mode::Backtest => historic_fill(setup, bar),                 // Faz 8.0a backtest
}
```

### 2.8 OCO bracket order builder
Wyckoff `TradePlan`'dan otomatik OCO grubu üretir:
- **Entry**: `LIMIT @ entry`
- **SL**: `STOP_MARKET @ entry_sl` (workingType=MARK_PRICE)
- **TP ladder**: her rung için `TAKE_PROFIT @ tp.price` size=qty_pct%
- Entry fill → SL+TP'ler aktive olur
- Fill kısmi → diğer OCO emirlerin qty'si güncellenir

### 2.9 Frontend `web-v2/src/pages/LiveExecution.tsx`
- Aktif live setup'lar paneli
- Circuit breaker durumu (open/tripped — kırmızı banner)
- Order state timeline her setup için
- Manual override: cancel order, force close
- API key health (testnet vs prod, last successful auth)

### 2.10 Konfig + Güvenlik
- API key'ler **kesinlikle DB'de değil** — `.env` veya OS keyring (referans config'de tutulur, key'in kendisi değil)
- `setup.live.enabled` master switch — false ise hiçbir live emir gitmez
- `setup.live.testnet_only = true` default — production geçişi explicit yapılacak
- `setup.live.max_position_usd_per_symbol` — tek pozisyon büyüklük cap'i
- `setup.live.allowed_venues = ["binance_futures_testnet"]` başlangıç

### 2.11 Test stratejisi
- **Unit**: order state machine, circuit breaker rules
- **Integration**: BinanceFuturesAdapter → testnet (gerçek API çağrı)
- **Chaos**: WS disconnect, rate limit, partial fill, stale position drift

### 2.12 Rollout sırası
1. ✅ Faz 8.0a Wyckoff dry+backtest tamamlanır + 4 hafta paper data toplanır
2. Faz 8.0c implement + testnet'te 2 hafta canlı tahta testi
3. **Tek sembol** + **tek profile (Q)** + **mikro pozisyon** ($10) ile prod
4. 1 ay başarılıysa pozisyon büyüklüğü kademeli artırılır
5. Tüm semboller + D+Q profile aktive

---

## 3. Risk Notları

- Live = **gerçek para kaybı riski**. Bug = $$$ kayıp.
- Circuit breaker'ı atlatabilecek tek satırlık kod bile kabul edilemez.
- API key sadece IP whitelist + futures-only + withdraw kapalı olacak.
- Audit log immutable (append-only) tablo, her order/cancel/circuit event'i.
- Disaster recovery: tüm açık pozisyonları "panic close" eden tek komut script'i.

## 4. Tahmini İş Yükü

| Kısım | Saat |
|---|---|
| Migration + ExecutionAdapter trait | 4 |
| BinanceFuturesAdapter (mevcut metodları sarmala) | 6 |
| Reconciliation + circuit breaker + state machine | 12 |
| OCO bracket builder + setup_loop integration | 8 |
| Frontend live panel | 6 |
| Test (unit + testnet integration) | 10 |
| **Toplam** | **46** (1.5–2 hafta full-time) |

---

## 5. CLAUDE.md Uyumu

- **Kural #1**: Adapter trait + dispatch table; circuit breaker rules early-return chain
- **Kural #2**: Tüm risk limitleri, retry sayıları, timeout'lar `system_config`'de
- **Kural #3**: Execution adapter pattern/strateji bilmez — `OrderRequest` alır, `OrderAck` döner
- **Kural #4**: `BinanceFuturesAdapter` venue-specific; `qtss-execution` core trait venue-agnostic
- **Kural #5**: Dry/Live/Backtest hepsi runtime context — feature flag yok, mode kolonu var
