# BUG — TP/SL projection üretiyor olabilir negatif fiyat

Durum: **backlog** (high — Telegram/GUI'de görünür hatalı setup).
Tarih: 2026-04-19

## Belirti
Telegram setup kartı ekran görüntüsü (RAVEUSDT SHORT, profil Q,
1h, "Selling High", skor 9/10):

```
Giriş : 1.18133
Stop  : 1.747431  (-47.92 %)
TP1   : -0.23392268  (+119.80 %)   ← negatif fiyat
R:R   : 2.50
```

Entry 1.18133, SL 1.747431 → SL mesafesi 0.566.
TP1 = entry − 2.5·R = 1.18133 − 2.5·0.566 = −0.232. Yani R-multiple
projeksiyonu fiyatı **0'ın altına itiyor**, kart o değeri olduğu gibi
formatlıyor. Gerçek market'te negatif fiyat fizikselen imkânsız —
projection sırasında bir alt sınır (`max(0, ...)` veya min-tick) kontrolü
yok.

## Etki
- GUI/Telegram'da güvenilirlik kaybı (operator "−0.23 hedef" görüyor).
- Watcher TP1'i ulaşılamaz olarak işaretliyor (close > 0 her zaman),
  setup düşüncesizce trail'a geçer veya açık kalır → P&L hesabı bozulur.
- Subscriber outbox bu kartı dağıtırsa dış görünüm de bozulur.

## Şüpheli noktalar (önce kontrol edilecek)
1. `crates/qtss-wyckoff/src/setup_builder.rs` → `pnf_target` (cause/effect
   factor × range height projeksiyonu): short tarafında
   `range_bottom − proj` yapılıyor; range_height büyükse 0 altına düşer.
2. `crates/qtss-wyckoff/src/trade_planner.rs` → R-multiple TP ladder:
   `entry − k·R` çarpanları büyük R'larda 0'ı geçer.
3. `crates/qtss-classical/src/...` veya generic structural target
   projeksiyonu (cup/H&S magnet): pattern-height shorts için aynı tuzak.
4. `qtss-setup-engine` hedef üretimi (T/Q/D profil ratchet'leri).

## Beklenen davranış
- **Hard floor**: hiçbir hedef/SL `min_price = max(tick_size, ε)` altına
  düşmemeli. Düşerse:
  1. O hedef elenir (TP3 düşerse TP2 son hedef olur),
  2. Eğer TP1 bile invalid → setup arm edilmez, `setup_rejection` yazılır
     (`reason = "negative_target_projection"`).
- **Telemetri**: rejection sayacı `qtss_v2_setup_rejections` üzerinden
  takip; aynı sembol/timeframe'de tekrar ediyorsa formation/strategy
  parametreleri (cause-effect factor vb.) çok agresif demektir.
- **Test**: per-builder unit test —
  ```
  fn projection_clamps_above_zero_for_short() {
      // entry 1.18, SL 1.74, range_height 0.6 → TP projeksiyonu < 0
      // → Some(rejected) veya filtered ladder
  }
  ```
  qtss-wyckoff/qtss-classical/qtss-setup-engine'in her birinde.

## Önerilen düzeltme yaklaşımı
1. `qtss-domain` veya `qtss-setup-engine`'e ortak `clamp_price_floor(p, tick) → Option<Decimal>` helper. None dönerse hedef invalid.
2. Tüm builder'lar bu helper'ı zorunlu çağırsın (tek kapı, CLAUDE.md #1
   dispatch tablosuna uygun).
3. Strategy katmanı (T/Q/D) ladder'ı kurarken None öğeleri eler; tüm
   ladder boşalırsa setup reject.
4. Outbox/Telegram render guard: `tp.iter().all(|t| *t > 0)` aksi halde
   "skip card" + `Sentry/log warn` ("setup id X TP'si 0'ın altına düştü,
   Faz 9.X bug — bug_negative_target_price.md").

## İlgili
- Bug raporu screenshot tarihi: 2026-04-19, RAVEUSDT SHORT 1h Q profile
- Subscriber kartı: bkz. ekran görüntüsü
- CLAUDE.md #1 (dispatch) + #2 (config) — clamp davranışı + min_tick
  config'ten okunsun (`risk.min_target_price` veya symbol catalog'dan
  `min_qty/min_price`).

## Tahmini efor
- Helper + entegrasyon: 3-4 saat
- Builder bazında testler: 2 saat
- Outbox guard + telemetri: 1 saat
- Toplam: ~yarım gün
