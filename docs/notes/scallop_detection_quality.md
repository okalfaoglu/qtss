# Scallop detection quality — audit + backlog

Tarih: 2026-04-19
Tetikleyici: BTCUSDT 1h scallop_bullish %69 confidence → RimR sonrası
ciddi retrace; hedef 1.0x bile gerçekleşmeden dönüş.

## Bulkowski J-shape vs. kodumuz
`crates/qtss-classical/src/shapes.rs::eval_scallop_side` (1514–1573):

| Kriter | Kod | Bulkowski | Durum |
|---|---|---|---|
| 3 pivot [H,L,H] / [L,H,L] | ✓ | ✓ | OK |
| Apex her iki rim'in karşı tarafında | ✓ | ✓ | OK |
| Rim progression | ≥ %2 (loose) | > %0 yeterli, ama breakout confirmation kritik | 2026-04-19 → %3.5'a sıkıldı |
| Parabolic R² | ≥ 0.55 (gevşek) | J-şekli belirgin (0.70+ tipik) | 2026-04-19 → 0.70'e sıkıldı |
| Min span | 20 bar | ~20–40 bar | OK |
| Volume confirmation | **YOK** | RimR breakout bar'ı avg üstünde | Eksik |
| Breakout teyit bar'ı | **YOK** | RimR'i close üstü kırmış bar | Eksik |

## 2026-04-19 düzeltilen
- `scallop_roundness_r2`: 0.55 → **0.70** (config.rs default + migration 0176)
- `scallop_min_rim_progress_pct`: 0.02 → **0.035** (config.rs default + migration 0176)
- **Breakout + volume confirmation (migration 0177)**:
  - `scallop_confirm_lookback = 5` — RimR sonrası max 5 bar aranıyor.
  - `scallop_breakout_atr_mult = 0.25` — close RimR'i en az 0.25·ATR aşmalı.
  - `scallop_breakout_vol_mult = 1.3` — breakout bar'ı son 20 bar avg vol × 1.3 üstünde.
  - `scallop_vol_avg_window = 20`, `scallop_atr_period = 14`.
  - Skor formülü: `0.35·s_round + 0.25·s_progress + 0.25·s_breakout + 0.15·s_volume`.

## Kalan backlog (ayrı başlık)

1. ~~Post-RimR breakout confirmation~~ ✅ 2026-04-19
2. ~~Volume confirmation~~ ✅ 2026-04-19
3. ~~Score formula rebalance~~ ✅ 2026-04-19
4. ~~Score weights → config~~ ✅ 2026-04-19 (migration 0179, runtime renorm)

### Kalan
5. **Backtest kalibrasyonu** — 6 ay historical BTCUSDT/ETHUSDT 1h+4h
   scallop hit-rate vs. Bulkowski ~%70 baseline. Runbook aşağıda.

## Kalibrasyon Runbook (Faz 10 Aşama 4.3)

Önkoşul: Migration 0178 (backtest dispatcher) + 0179 (score weights)
prod'a uygulandı. `backtest.setup_loop.enabled = true`.

### 1. Detection + setup birikmesini bekle
```sql
-- Scallop backtest setup sayısı (min 100 kapanmış örnek hedefi)
SELECT state, COUNT(*)
  FROM qtss_setups
 WHERE mode = 'backtest'
   AND alt_type LIKE 'scallop_%'
 GROUP BY state;
```

### 2. Baseline hit-rate ölç
GUI: `/v2/backtest` → `alt_type LIKE` filtresine `scallop_%` gir.
Beklenen: `hit_rate ≥ 0.60`, `avg_pnl_pct > 0`. Bulkowski ~%70 referans.

SQL eşdeğeri:
```sql
SELECT
  COUNT(*) FILTER (WHERE pnl_pct > 0) AS wins,
  COUNT(*) FILTER (WHERE pnl_pct < 0) AS losses,
  AVG(pnl_pct)                         AS avg_pnl,
  SUM(pnl_pct)                         AS total_pnl
FROM qtss_setups
WHERE mode = 'backtest'
  AND state = 'closed'
  AND alt_type LIKE 'scallop_%';
```

### 3. Confidence tier kalibrasyonu
Score'un gerçek başarıyla hizalı olduğunu doğrula:
```sql
SELECT
  width_bucket(COALESCE((raw_meta->>'score')::float, 0.5), 0, 1, 5) AS score_bin,
  COUNT(*)                                                         AS n,
  AVG((pnl_pct > 0)::int::float)                                   AS hit_rate
FROM qtss_setups
WHERE mode='backtest' AND state='closed' AND alt_type LIKE 'scallop_%'
GROUP BY 1 ORDER BY 1;
```
Beklenen: bin monotonik artıyor (yüksek score → yüksek hit-rate).

### 4. Ağırlık sweep (tier'lar monoton değilse)
Config GUI'den module=`detection`, anahtarlar:
`classical.scallop_score_w_{curvature,progress,breakout,volume}`. Tek
seferde bir ağırlığı ±0.10 oynat, 24h örneklem sonra 2. adımı tekrarla.
Detector runtime'da renormalise eder — toplam 1.0 olmak zorunda değil.

### 5. Eşik kesintisi
Hit-rate yeterince doygunsa `min_structural_score` veya Strategy
tarafındaki confidence floor'u yükselt (şu an `0.50`).

### Kabul kriteri
- ≥ 100 kapanmış scallop backtest setup
- `hit_rate ≥ 0.60` (tercihen 0.65+)
- score tier'ları monoton
- `total_pnl_pct > 0`

Hedefler tutmuyorsa follow-up faz: pattern-height projeksiyonunu
gevşet / invalidation yaklaşımını gözden geçir.

## Notlar
- Skor ağırlıkları `system_config` altında (migration 0179).
- Detector runtime renormalise eder, toplam 1.0 olmak zorunda değil.
- Bu değişikliklerden sonra scallop **detection sayısı düşer** (breakout/volume gating) ama **detection başına güven artar**: hayatta kalan tespitler zaten mekanik tetiği geçmiş olur. Skor dağılımı da s_breakout + s_volume eklendiği için gerçek başarı oranını daha iyi yansıtır.

