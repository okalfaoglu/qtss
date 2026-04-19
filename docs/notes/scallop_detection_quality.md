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

### Kalan
4. **Backtest kalibrasyonu** — 6 ay historical BTCUSDT/ETHUSDT 1h+4h scallop hit-rate vs. Bulkowski ~%70 baseline. Confidence tier'larının realized başarı oranını yansıttığını doğrula.

## Notlar
- Skor formülü ağırlıkları (`0.35/0.25/0.25/0.15`) henüz config'te değil — kod düzeyinde sabit. Yeterli backtest verisi toplandıktan sonra ağırlıkları config'e taşıyacağız (CLAUDE.md #2).
- Bu değişikliklerden sonra scallop **detection sayısı düşer** (breakout/volume gating) ama **detection başına güven artar**: hayatta kalan tespitler zaten mekanik tetiği geçmiş olur. Skor dağılımı da s_breakout + s_volume eklendiği için gerçek başarı oranını daha iyi yansıtır.

