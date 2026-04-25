# FAZ 25.2 — Elliott State Machine: design proposal

User feedback (2026-04-26): "abc hala simülasyonda kalmış. simülasyon
gerçek hayattan kopuk ilerliyor. simülasyonda gerçekleştiğinde o artık
elliot olmuştır. simülasyon aynı zamanda gerçek mumlarıda takip
etmelidir. yapı daha da sağlamlaştıracak aynı zamanda elliot ilede
konuşacak bir yapı kurma yönünde ekstra düşün."

This doc captures the architecture proposal that came out of that ask.

## Bugün ne var

Üç yazıcı + bir tracker, hepsi `detections` üzerinden konuşuyor:

```
elliott.rs          → motive / abc / triangle (Pine port'un kendi durum makinesi)
elliott_early.rs    → impulse_nascent/forming, abc_projected/nascent/forming, extended
elliott_full.rs     → diagonal / flat / extended / truncated / combination
                      (qtss-pivots L1 üzerinden)

iq_structure_tracker_loop → detections'tan okuyor, iq_structures tablosuna
                            durum makinesi materialise ediyor (W1..W5/A/B/C +
                            candidate/tracking/completed/invalidated)
```

İki aynı sembol için iki farklı "kişilik" üretiyor:
- `elliott_early` her tick yeniden hesaplıyor, statik projection emit ediyor
  (bar_index'ler geleceğe doğru extrapolate)
- `iq_structures` durum makinesi tutuyor ama detections'a feedback vermiyor —
  sadece pasif okuyucu

Bug: simülasyon ileriye doğru bar üretirken gerçek mumlar oraya geliyor,
ama simülasyon "gerçekleşti, demek ki Elliott oldu" diyemiyor. İki kaynak
arasında geri besleme yok.

## Hedef mimari

Kanonik durum makinesi `iq_structures` olsun, `elliott_early`'nin emit ettiği
projection'lar OKUNAN değer (canlı durum) — yazılı kalıcı detection değil.

```
                    ┌───────────────────────────────┐
                    │  Pine port + qtss-pivots       │
                    │  (raw pivot tape)              │
                    └────────────┬───────────────────┘
                                 │
                                 ▼
                    ┌───────────────────────────────┐
                    │  ElliottContextManager         │
                    │  per (sym, tf, slot)            │
                    │  ─ FSM: Idle → Impulse(Wn) →   │
                    │         CorrectiveProj →        │
                    │         CorrectiveActive(A,B,C)│
                    │         → Done | Invalidated    │
                    │  ─ Anchor list = REAL pivots    │
                    │  ─ Projection = hesaplanan,     │
                    │    yalnızca o anki state için   │
                    └────────────┬───────────────────┘
                                 │
                ┌────────────────┼─────────────────┐
                │                │                 │
                ▼                ▼                 ▼
        iq_structures      detections          live SSE
        (materialised)     (history audit)     (chart push)
```

## Anahtar prensipler

### 1) Tek "current state" per (sym, tf, slot)
Detections tablosuna her tick'te 4 satır (projected + nascent + forming + ...)
basmıyoruz. Her tuple için TEK bir state row var: `iq_structures`. Geçmiş
durumlar history için tutulur (önceki state'in kapanış zamanı).

### 2) Anchor = sadece gerçek pivot
Projection ASLA anchor olarak yazılmaz. `iq_structures.structure_anchors`
sadece gerçekleşen pivot'ları içerir. Projection canlıdan hesaplanır:
read-time'da iq_structure'ın current_wave'ine göre frontend / API hangi
projeksiyonların çizileceğini bilir.

```rust
// Read endpoint pseudocode
match (current_wave, current_stage) {
    ("W3", "nascent")  => project_w4_w5(anchors),
    ("W5", "completed") => project_abc(anchors, last_bar),
    ("B",  "forming")   => project_c_only(anchors, last_bar),
    ("C",  "completed") => no_projection,  // cycle done
    _ => no_projection,
}
```

### 3) Projection = current_bar'a clip'lenir, asla geçmişe geri yazılmaz
Bugünkü cerrahi fix bunu zaten uyguladı (clip_to_last). Mimaride bu kalıcı
bir sözleşme: read-time projection function her zaman `target_bar.min(last_bar)`
döner.

### 4) Promotion = gerçek pivot çapayı içine alınca otomatik
Her tick:
1. Yeni pivot'lar (Pine port + mini-pivot merge) listele
2. Önceki state'in beklediği bir sonraki çapayla eşleşiyor mu kontrol et
   (örn. state=W3-projected, expected_next=W4-low → bir LOW pivot W3
   altında ve W2 altında değilse → eşleşti)
3. Eşleştiyse: state'i ilerlet (W3 → W4), anchors'a o pivot eklenir,
   projection geçmişten silinir
4. Eşleşmediyse: aynı state, sadece projection bar_index güncellenir

### 5) Invalidation = first-class transition
Her state'in `invalidates_on(...)` predikatı var:
- W3-projected, expected W4: price W2'yi kırarsa → invalidated
- W5-completed, expected ABC: price W4'ü kırarsa (zigzag) veya extension
  girerse → state'i Extension'a geçir, ABC simulasyonu sil
- B-forming, expected C: price A'nın ekstremumunu yeniden test ederse,
  triangle/wxy alt-tip dener

Şu anki bug bu invalidation'ın `elliott_early` içinde bağımsız çalışması;
mimari iq_structures'a alıyor.

### 6) Çıktı kontratları
```rust
// API: /v2/iq-structure/{venue}/{symbol}/{tf}/{slot}
{
  "id": "...",
  "current_wave": "W3",
  "current_stage": "nascent",
  "state": "candidate",
  "anchors": [...],          // gerçek pivot'lar
  "projection": {
    "next_wave": "W4",
    "anchors": [             // simülasyon, her biri canlı clip
      { "label": "W4?", "bar_index": 1234, "price": 71500.0 }
    ],
    "horizon_bars": 5,
    "fib_band": {...}
  },
  "invalidation": {
    "trigger_price": 67500.0,
    "rule": "W4 below W1 — overlap"
  }
}
```

Frontend tek bu endpoint'ten okur, state'e göre dotted/solid/labeled
çizer. Iki kaynaktan veri alıp uzlaştırma yok.

### 7) Çoklu yorum (zigzag/flat/triangle/combination paralel)
Bir motive bittiğinde N farklı corrective senaryosu olabilir. Mimari her
senaryoyu PARALEL track eder:

```
W5 completed
├── path: zigzag (50% A, B at 0.5*A, C = A − (B−A))
├── path: flat_regular (B ≈ A, C ≈ A)
├── path: flat_expanded (B = 1.272*A, C = 1.618*A)
├── path: triangle (5 alt-leg)
└── path: combination_wxy
```

Her path için `confidence_score` taşınır. Yeni pivot geldiğinde her path
yeniden değerlendirilir, bazıları invalidate olur, bazılarının
`confidence_score`'u artar. UI default olarak en yüksek confidence path'i
çizer; "all paths" toggle ile diğerleri açılır.

## Gerçekleştirme planı (incremental)

### Faz 25.2.A — bu PR (acil cerrahi)
Bugünkü değişiklikler:
- `elliott_early.rs` post_w5 = (Pine port ∪ mini-pivot) merged set,
  first_valid filter direction-only-once-on-merged
- Projected bar_index'ler `clip_to_last` ile current_last_bar'a kapatıldı
- Sonuç: simülasyon bugünkü mum sınırını aşmıyor; mini-pivot Pine
  port'un kaçırdığı opposite-direction swing'leri yakaladığı için
  abc_projected'tan abc_nascent'e geçiş daha hızlı

### Faz 25.2.B — durum makinesi entegrasyonu
- `iq_structure_tracker_loop` her tick'te scan yapmak yerine event-driven
  hale gelir: yeni pivot geldiğinde state transition kontrolü
- `elliott_early.rs` projection-only emit etmeyi bırakır; sadece
  iq_structures'ı besler
- API: `/v2/iq-structure/{...}` endpoint'i tek source of truth olur
- Frontend `LuxAlgoChart`: detections + iq_structure birlikte okur, iq_structure
  current_wave'ine göre projection katmanını clip eder

### Faz 25.2.C — paralel hipotez izleme
- `iq_structure_branches` (yeni tablo): her motive'in her corrective
  yorumu (zigzag/flat_regular/flat_expanded/flat_running/triangle/wxy)
  ayrı row
- Her tick yeni pivot ile her branch'in confidence_score'u güncellenir
- Branches arası dedup: en yüksek skorlu branch chart'a düşer, diğerleri
  "alternative" overlay olarak

### Faz 25.2.D — tier-based entry & PositionGuard integration
- iq_d_candidate_loop iq_structure'dan W1/W2/W3 tier'larını okur
  (zaten okuyor — bugün düzeltildi)
- Yeni: branch hipotezi seçilince entry tier'lar branch'e göre değişir.
  Örn: flat_expanded'de W3 entry, zigzag'da olduğundan farklı seviyede

## Bu PR'da ne var, ne yok

**Var (FAZ 25.2.A — surgical)**
- Pine port + mini-pivot post-W5 merge (her zigzag için geçerli)
- Projected bar clip-to-last
- Build + restart + verification

**Yok**
- Durum makinesi konsolidasyonu (Faz 25.2.B)
- Paralel hipotez izleme (Faz 25.2.C)
- Tier-branch entegrasyonu (Faz 25.2.D)

Bunlar ayrı PR'larda. Önce A çalıştığını canlı doğrulayalım, sonra B'ye
geçeriz. Toplam 4 sprint civarı iş — staleness gate + iq-d entry fix gibi
"acil" değil; doğru kurmak gerekir.
