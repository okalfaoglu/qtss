# BUG — backtest modunda setup üretilmiyor

Tarih: 2026-04-19
Durum: **açık**, sonra ele alınacak.

## Belirti
`qtss_v2_detections` tablosunda **453k+ backtest detection** var ama
`qtss_setups` tablosunda `mode = 'backtest'` satırı **0** (veya çok
az). Live/dry modları normal çalışıyor.

## Root cause (kısmen anlaşıldı)
`v2_setup_loop` sadece live + dry detection'larını dinliyor. Backtest
detection'ları için ayrı bir dispatcher yok — detection'lar
`qtss_v2_detections`'a yazılıyor ama setup engine'e hiç akmıyor.

## Başlanmış scaffolding (uncommitted → committed 2b65e8a)
- `crates/qtss-storage/src/v2_detections.rs::list_backtest_unset_detections(limit)`
  LEFT JOIN ile setup'a bağlanmamış backtest detection'larını getiriyor.
  Şu an kullanılmıyor — loop tarafı yazılmadı.

## Gerekli iş (sonra)
1. **Yeni worker loop**: `crates/qtss-worker/src/v2_backtest_setup_loop.rs`
   - `list_backtest_unset_detections`'tan batch oku
   - Her detection için `try_arm_new_setup`'ı `mode = "backtest"` ile çağır
   - Allocator zaten mode-scoped (commit 9610cd0) — backtest slot havuzu ayrı
2. **Confluence mode-awareness**:
   - `qtss_v2_confluence` tablosunda bazı veri tipleri (regime, ADX)
     live üzerinden dolduruluyor. Backtest için historical snapshot'tan
     hesaplanmalı. Aksi halde backtest confluence boş → setup arm edilmez.
3. **Migration**:
   - Muhtemelen yok — schema zaten `mode` kolonunu destekliyor (commit 9610cd0).
   - Sadece backtest loop'un aktif olduğunu işaret eden bir config flag: `backtest.setup_loop_enabled`.
4. **Main.rs wiring**:
   - `qtss-worker/src/main.rs`'te yeni loop'u `tokio::spawn` ile başlat.
   - Config flag kapalıysa atla.
5. **Test**:
   - Fixture: 5 backtest detection → loop 5 setup üretmeli, mode = backtest.
   - Live loop aynı anda çalışırken birbirine karışmadığını gösteren integration test.

## Tahmini efor
~300 LOC + 1 migration + 2-3 test.

## Niye şimdi değil
Kullanıcı 2026-04-19'da classical target + GUI işine pivot etti.
Backtest dispatcher'ı bekletildi çünkü blocker değil — live pipeline
AI training için yeterli veri üretiyor, backtest veri hacmini
büyütmek "nice to have".

## İlgili commit'ler
- `2b65e8a` — scaffolding (list_backtest_unset_detections)
- `9610cd0` — allocator mode-scoping + v2_setups.mode kolonu
- `e9574c0` — CloseReason DB constraint fix
