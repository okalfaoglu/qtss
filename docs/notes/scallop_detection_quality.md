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

Bunlar `system_config` tablosunda, worker restart'ta veya config-reload sonrası etki eder.

## Kalan backlog (ayrı başlık)

1. **Post-RimR breakout confirmation**
   - Detector şu an RimR pivot oluşur oluşmaz tetikliyor.
   - Gerekli: RimR bar'ından sonra en az 1 bar, RimR fiyatını `N * ATR` üstünden close etmeli (bull için).
   - Implementasyon: `eval_scallop_side` imzasına `bars[pivots[2].bar_index + 1 ..]` slice'ı aktarılmalı, confirmation eşiği config'e `scallop_breakout_atr_mult` olarak girmeli.
   - Risk: pivot'lar sadece kapanmış swing'ler olduğu için zaten gecikmeli tetikleniyor; breakout confirmation ekstra 1–3 bar daha geciktirir ama false positive'i ciddi biçimde düşürür.

2. **Volume confirmation**
   - RimR breakout bar'ının hacmi son N bar ortalamasının ≥ 1.3× olmalı.
   - Config: `scallop_breakout_vol_mult` (default 1.3).
   - Bar'larda `volume` alanı zaten mevcut; `bars[pivots[2].bar_index].volume` ile `avg(bars[pivots[2].bar_index - 20 ..])`.

3. **Confidence tier'larına kalibrasyon**
   - Scallop skoru şu an `(s_round + s_progress) / 2` — breakout + volume teyidi geldiğinde skora katılmalı:
     ```
     score = 0.35*s_round + 0.25*s_progress + 0.25*s_breakout + 0.15*s_volume
     ```
   - Aksi takdirde confidence dağılımı gerçek başarı oranını yansıtmıyor.

4. **Backtest kalibrasyonu**
   - 3 madde uygulandıktan sonra 6 ay historical BTCUSDT/ETHUSDT 1h + 4h scallop detection hit-rate ölç. Bulkowski avg ~%70 civarı referans.

## Önerilen sıra
(1) ve (2) beraber gider (aynı fonksiyon imzası değişikliği). (3) ve (4) sonra.
