# Faz — Formation Quality & Completion Engine (backlog)

Tarih: 2026-04-19
Durum: **backlog** — yeni faz olarak eklendi, sonra konuşulacak.
Tetikleyici: BTCUSDT 1h `flat_expanded_bull` %66 — TP1 (MM 1.0x 77402) hit,
TP2'ye (MM 1.618x 78510) gelmeden ciddi retrace. Formasyon "tamamlandı"
mı, "kısmen başarılı" mı, "başarısız" mı — şu an kodda cevap yok.

## Kapsam (kullanıcı talebi, özet)
1. Her formasyon için **kural gözden geçirmesi** (Bulkowski / Pesavento /
   Elliott / Wyckoff vs. kodumuz karşılaştırması, scallop örneğindeki gibi).
2. Formasyon doğruluğunu artıracak **ek kontroller** (confirmation
   gate'leri, volume, trend, multi-TF konteyner kontrolü, geometrik
   tolerans sıkılaştırma). Her formasyon için ayrı araştırma + adım
   listesi.
3. Her formasyonun **hedef değerleri + başarı kriterleri** AI'a feature
   olarak gönderilmeli (şu an sadece target_count / has_structural_targets
   / target_1_r gidiyor — başarı sonucu gitmiyor).
4. **Completion/Outcome Engine** — oluşan formasyonları canlı takip
   edip TP hit / SL hit / timeout / partial success outcome'unu yazan
   ayrı worker (target_followup_loop benzeri ama formation-level).
5. AI entegrasyonu — bu yapı AI'a mı bilgi veriyor, yoksa ayrı bir
   channel mı? (Tartışılacak; muhtemelen `ai_training_snapshot` tablosuna
   outcome kolonu eklemek yeterli.)
6. **GUI**: Tamamlanan formasyonlar default Chart'ta gözükmesin; ayrı
   bir "Tamamlanan Formasyonlar" butonu/paneli — başarı/başarısızlık
   rozetiyle. Kullanıcı gerektiğinde görseli gönderip analiz isteyecek.

## Bugünkü örnek (tartışma için referans)
`flat_expanded_bull` 66% confidence, entry 75425.60, SL 73256.80,
TP1 77402.20 (MM 1.0x), TP2 78510.50 (MM 1.618x):
- Fiyat TP1'i net kırdı → 77900 civarı tepe → 75500'e retrace.
- Şu anki kodda bu formasyon hâlâ "aktif" sayılıyor (completion state
  machine yok). Setup tarafında PositionGuard ratchet TP1 hit sonrası
  SL'i entry'e çekiyor olabilir (setup.rs'deki logic) ama **detection
  seviyesinde** formation'ın "completed / partially_successful"
  etiketlenmesi yok.

**Kullanıcı sorusu**: "TP1 hit + TP2 miss + retrace" durumunda formasyon
tamamlandı denebilir mi, %kaç başarılı?

**Önerilen tanım** (tartışılacak):
- **Success = 1.0**: TP_final hit → SL'e dönmeden.
- **Partial = TP_n_weight toplamı**: hit olan TP'lerin `weight` toplamı /
  toplam weight. Örn TP1 hit (0.70) + TP2 miss (0.45) →
  0.70 / (0.70+0.45) = **%60.8 partial success**.
- **Failure = 0.0**: Hiç TP hit olmadan SL.
- **Timeout**: `time_stop_secs` doldu, ne TP ne SL → partial w/ current-MFE.
- **Completion kriteri**: (a) TP_final hit, veya (b) SL hit, veya (c)
  TP_n hit sonrası fiyat entry'nin altına dönüp `N * ATR` kadar kaldı
  (tutunamadı → "partially_completed_then_reversed"), veya (d) timeout.

## Mevcut kodda ne var / ne yok
**VAR:**
- `qtss_v2_detections` tablosunda `state` kolonu: Forming / Confirmed /
  Invalidated / Completed. Ama "Completed" şu an **sadece** Elliott
  full 5-wave için set ediliyor; classical ve harmonic için set edilmiyor.
- `raw_meta.structural_targets = [{price, weight, label}]` — artık her
  formation için yayılıyor (commit 8ca744a).
- `setup_target_followup_loop` — setup seviyesinde TP hit/miss izler
  (setup.tp_hit_mask); detection seviyesinde eşdeğeri yok.
- PositionGuard ratchet — TP hit sonrası SL'i ileri çeker (setup.rs).

**YOK:**
- Detection-level completion state machine.
- "Partial success score" hesabı.
- Tamamlanan formasyonları saklayan / gösteren ayrı tablo + UI.
- AI training snapshot'ına `outcome_label`, `outcome_score`,
  `hit_target_count`, `mfe_pct`, `mae_pct`, `time_to_outcome_bars` feature'ları.
- Per-formation rule audit (scallop hariç; o 2026-04-19'da yapıldı).

## Önerilen yapı taşları (detay sonra konuşulacak)
1. **Per-formation spec dosyaları** (docs/formations/*.md): her biri
   için Bulkowski/literatür kuralları, kodumuzdaki karşılığı, gap'ler,
   sıkılaştırma adımları. Scallop notu (docs/notes/scallop_detection_quality.md)
   template.
2. **FormationOutcomeEngine worker**: `crates/qtss-worker/src/formation_outcome_loop.rs`.
   - Forming/Confirmed detection'lar için tick-by-tick veya bar-by-bar
     TP/SL kontrol.
   - Completion → `qtss_v2_detection_outcomes` tablosuna yaz
     (detection_id, outcome_label, outcome_score, hit_targets,
      mfe_pct, mae_pct, time_to_outcome, closed_at, reason).
   - CLAUDE.md #2: tüm eşikler (retrace_invalidation_atr_mult,
     timeout_bars_multiplier, partial_success_threshold) config'te.
3. **AI feature extension**: `ClassicalSource`/`HarmonicSource`/
   `ElliottSource` snapshot'larına outcome lookup ekle (retrospektif:
   bir detection kapandığında bağlı snapshot'ı outcome ile güncelle,
   veya training loader join ile çeker).
4. **GUI**:
   - Chart'ta default olarak sadece `Forming`+`Confirmed` çizilsin
     (şu an `Completed` olan yok ama ileride olacak).
   - Yeni toolbar butonu: **"Tamamlanan"** — tıklayınca son N gün
     completed detection'ları liste + mini thumbnail + outcome rozeti
     (yeşil %100, sarı %60 partial, kırmızı %0).
   - Detay panelinde outcome gerekçesi ("TP1 hit, TP2 miss, reversed to
     entry-1.5·ATR after 12 bars").
5. **Kural audit sırası** (öneri — tartışılacak):
   - Öncelik 1: yanlış-pozitif yüksek olanlar (scallop ✅, flat, zigzag,
     triangle, double top/bottom).
   - Öncelik 2: harmonic (Pesavento tolerans sıkılaştırma, Gartley vs
     Bat vs Butterfly ayrım gücü).
   - Öncelik 3: Elliott impulse/corrective (zaten tight — son).

## Açık sorular (kullanıcıya sormak için)
1. Partial success formülü: weight-toplamı mı yoksa `max_hit_target_R /
   final_target_R` mi? (İkinci daha konservatif.)
2. "Tamamlandı" için timeout: formation span'ı × K mı, yoksa TF-bazlı
   sabit bar sayısı mı?
3. "Completed" detection'lar AI training'e **ayrı bir kolon** olarak
   mı girecek (outcome_label), yoksa tamamen ayrı bir
   `formation_outcomes` tablosundan mı join edilecek? (Önerim: ayrı
   tablo, training loader'da join.)
4. Reversal invalidation: TP_n hit sonrası fiyat entry altına
   `M * ATR` dönerse "reversed" label → M config'te. Default?
5. GUI "Tamamlanan" panel: kullanıcı bazlı mı (her kullanıcı farklı
   gün aralığı seçebilir), global mi?

## İlgili dosyalar (start kit)
- `crates/qtss-worker/src/v2_setup_loop.rs::compute_structural_targets_raw`
  (~satır 1200) — target üretiminin kalbi.
- `crates/qtss-classical/src/shapes.rs` — her classical formation eval.
- `crates/qtss-harmonic/src/` — XABCD formations.
- `crates/qtss-elliott/src/` — impulse/corrective.
- `web-v2/src/pages/Chart.tsx::computeFormationTargets` — TS mirror.
- `crates/qtss-ai/src/sources/classical.rs` — ClassicalSource feature emit.

## Tahmini efor (kaba)
- Kural audit: formation başına 1-2 saat araştırma + 2-4 saat kod
  sıkılaştırma. ~15 formation × ~4 saat ≈ 60 saat.
- Outcome engine: ~600 LOC + 1 migration + test. ~2 gün.
- GUI panel: ~300 LOC + API endpoint. ~1 gün.
- AI feature extension: ~150 LOC + snapshot migration. ~4 saat.

## Niye şimdi değil
Kullanıcı "mevcut işini kesme, sonra konuşacağız" dedi. Bu faz
büyük bir iş ve önce yön kararı verilmeli (partial success formülü,
AI pipeline mimarisi, GUI akışı). Şu anki Wyckoff/Chart fix'leri
bitince bu dokümanı baz alarak sohbet başlayacak.
