# GUI — hedef ve faz doğruluğu (takip notu)

Tarih: 2026-04-19
Kullanıcı raporu: BTCUSDT 5m, Wyckoff overlay + setup overlay birlikte.

## Gözlem (ekran görüntüsüne göre)
- **Market Phase kartı**: "RANGE · redistribution?" yazıyor ama alt satırda "Phase A" gösteriliyor. Redistribution Phase A ↔ UI metni tutarsız. Ayrıca fiyat açıkça aşağı kırılmış (75k'ye düşmüş) — UI hâlâ "range" diyor.
- **SPRING etiketleri**: Düşüş yapısının içinde Spring işaretlenmiş. Spring = accumulation Phase C event; redistribution'da karşılığı "Upthrust (UT/UTAD)". Yani ya faz yanlış, ya event sınıflandırması yanlış.
- **Hedefler terslik içeriyor**:
  - Entry 74951.60, SL 77046.39 (üstte) → **SHORT** setup gibi görünüyor.
  - TP1 75408.50 (entry'nin ÜSTÜNDE), TP2 74444.48, TP3 73848.60 (entry'nin ALTINDA).
  - TP1 yanlış tarafta. Ya measured-move hesabı yanlış işaret kullanıyor ya da target_engine'in long/short branch'i karışmış.

## İncelenecek noktalar
1. `qtss-target-engine::compute_structural_targets` — direction'a göre TP sıralaması.
2. `process_symbol` / `PositionGuard::new_structural` — entry/sl/tp tutarlılığı (SL entry'nin üstündeyse yön SHORT olmalı, TP'ler de altta).
3. `qtss-wyckoff` — phase detector (A/B/C/D/E) ve accumulation vs redistribution ayrımı.
4. Web GUI Wyckoff overlay:
   - Faz kartındaki "redistribution?" soru işaretli label nereden geliyor?
   - Event pin'leri (Spring/UTAD/SC/BC...) phase'e göre renk/metin seçiyor mu?
5. Setup overlay TP label'ları — "TP1/TP2/TP3" dizilişi entry'den uzaklık yerine sabit index'e bağlı olabilir; direction-aware sırala.

## Ayrı başlıklar (kullanıcı talebi, 2026-04-19)
- **Setups overlay on Chart**: armed/live setup'ları Chart'a çizme işi ayrı başlık.
- **Açık pozisyonlar GUI**: broker'da açık olan işlemleri Chart'a/Setups'a render etme ayrı başlık.

## 2026-04-19 ilerleme — Wyckoff faz/event tutarsızlığı
- **Market Phase kartı**: Chart.tsx'te "RANGE · redistribution?" yerine artık Phase A/B'de `RANGE · PHASE A` (direction commit edilmemiş) yazıyor. Phase C+ lock olduğunda ACCUMULATION/DISTRIBUTION headline'a çıkıyor. Detail satırındaki "Phase A" ile çelişki kalktı.
- **Spring/UTAD on ters schematic**: `WyckoffStructureTracker::record_event_with_time` artık Phase ≠ A iken event'in yönü committed schematic'i çelişkiye düşürüyorsa **ve** `auto_reclassify` hysteresis guard'ları flip'i engelliyorsa event'i reddediyor. Yeni helper'lar: `would_contradict_schematic(event)`, `reclassify_blocked(bar_index)`. Böylece `events_json` içinde Distribution fazında Spring (ya da Accumulation fazında UTAD) birikmesi engellendi — overlay artık yön uyumsuz pin çizmeyecek. 31 wyckoff test pass.

## 2026-04-19 ilerleme
- Backend artık her setup için `raw_meta.structural_targets = [{price, weight, label}, ...]` ve `raw_meta.structural_subkind` yayıyor (v2_setup_loop.rs). Formation'a özel etiketler ("MM 1.0x", "Pat 1.618x", "ABCD 1.272x") burada.
- Chart.tsx artık `computeFormationTargets(d)` ile backend `compute_structural_targets_raw` mirror'u kullanıyor. Her classical formation (double_top/bottom, H&S, triple_top/bottom, ABCD, V-spike, wedge/channel/triangle/rectangle/diamond/broadening/cup&handle/rounding/scallop), harmonic XABCD ve Elliott impulse için entry/SL/TP'ler formasyona özel label'larla ("MM 1.0x", "ABCD 1.272x", "Pat 1.618x", "AD 0.618") çiziliyor. TS mirror backend ile manuel senkron tutuluyor; backend unit-tested → truth source.
- AI (ClassicalSource) artık `target_count`, `has_structural_targets`, `target_1_r/weight`, `target_2_r/weight` feature'larını snapshot'a yazıyor.

## Beklenen davranış
- Entry tarafı SL ile aynı yönü paylaşıyorsa (SL üstte → long) TP'ler **altta** olmamalı; tam tersi.
- Phase label + sub-label (accumulation vs redistribution) tek kaynaktan gelmeli; UI boyunca tutarlı olmalı.
- Spring/UTAD event pin'leri phase'in tipine göre filtrelenmeli (accumulation fazında Spring; redistribution'da UTAD).
