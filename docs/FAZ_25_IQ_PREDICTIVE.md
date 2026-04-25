# FAZ 25 — IQ-D / IQ-T Elliott Predictive Layer

> **Durum:** Tasarım locked — implementation başlıyor (2026-04-25).
> Önceki kararlar bu konuşmada netleştirildi; aşağıda sallamadan, doğrudan
> kullanıcı onaylı maddelerle yazıldı.

---

## 0. Sıkı İzolasyon İlkesi (kullanıcı talebi)

**Mevcut T ve D profile'larına dokunulmaz.** IQ-D ve IQ-T tamamen
**yeni, paralel** profile'lar olarak eklenir:

- `qtss_setups.profile` mevcut değerleri (`t`, `d`) **olduğu gibi kalır**
- IQ-D / IQ-T için yeni değerler eklenir: `iq_d`, `iq_t`
- Mevcut allocator gate'leri, scorer ağırlıkları, setup_watcher path'leri, telegram render'ı — **hiçbiri bozulmaz**
- Mevcut migration ve config seed'ler — **dokunulmaz**
- Yeni kod paralel yol izler; bir sembolde aynı anda hem T-setup hem IQ-T-setup yaşayabilir

Regresyon riskini sıfırda tutmak için her PR'da **eski profile'lara
sadece "ek" değil, "değişiklik yok"** kuralı geçerli.

## 1. Amaç

Elliott dalgalarının **bittiği ve başladığı** noktaları bulup iki bağımsız
setup tipiyle işlem açmak:

- **IQ-D (Major):** büyük dip ve tepe avcısı, büyük TF, az ama yüksek-EV trade
- **IQ-T (Tactical):** IQ-D'nin yapısı içindeki düzeltmelerde tepki
  noktalarını avlayan küçük-TF setup

İki setup **bağımsız pozisyon**, **bağımsız SL/TP**, ama **ortak structure
context** paylaşır.

## 2. Kullanıcı kararları (2026-04-25 konuşmasından)

| # | Konu | Karar |
|---|------|-------|
| 1 | Yapı kapsamı | İtki(5) + ABC(3) = 8 dalgalı tam siklus |
| 2 | IQ-D entry önceliği | W1 > W2 > W3 (asıl hedef W1) |
| 3 | Yön | Çift yönlü — bullish ve bearish IQ-D yapıları |
| 4 | Pozisyon ayrımı | IQ-D ve IQ-T tamamen ayrı pozisyon, ayrı SL/TP/PnL |
| 5 | IQ-T yön kuralı | **Bir sonraki beklenen dalganın yönüne göre** (parent yönüne göre değil) |
| 6 | IQ-T tetik noktaları | W2-end, W4-end, sub-W2/W4 (W3,W5 içi), ABC'de A-end, B-end, C-end |
| 7 | TF kaskat oranı | Parent:child 1:8 — 1:30 |
| 8 | Veri katmanı | Multi-degree ZigZag (mevcut) + pivot-based wave bars (yeni — ileri faz) |
| 9 | Yapı bozulduğunda IQ-D | IQ-D **her durumda** iptal — o sembol **lock**, yeni IQ-D çıkana kadar yeni işlem yok |
| 9b | Yapı bozulduğunda IQ-T | IQ-T'nin **kendi mikro yapısı** da bozulduysa iptal; bozulmadıysa **temkinli hedef takibi** (SL break-even'a, partial TP erken, pozisyon kısalt) |
| 10 | Sizing | IQ-T sizing ≈ IQ-D sizing × 1/4–1/3 (sayı veriyle ayarlanacak) |
| 11 | Invalidation kuralları | Standart Elliott: W2 100% retrace, W4–W1 overlap, W3-shortest |

## 3. IQ-D Yapı Definisyonu

Bir IQ-D yapı bir tam Elliott siklusu izler:

```
Bullish IQ-D (long structure):
  W1 (up)  →  W2 (down)  →  W3 (up)  →  W4 (down)  →  W5 (up)
   →  A (down)  →  B (up)  →  C (down)
   →  [yeni W1 başlangıcı veya invalidate]

Bearish IQ-D (short structure): tam ayna görüntü
```

## 4. IQ-D Entry Politikası

| Tier | Tetik anchor | Tetik koşulu | Confluence eşiği |
|------|--------------|--------------|------------------|
| W1 | Önceki yapı bitmiş, yeni W1 oluşmaya başlamış | Lower-TF nascent impulse formed + previous structure complete | 6+ onay |
| W2 | W1 + W2 oluşmuş, W3'e binmek için | NascentImpulseDetector hit (4 pivot) | 5+ onay |
| W3 | W3 W1'i kırmış ama W4 daha gelmemiş | NascentImpulseDetector confirmed (W3 > W1) | 4+ onay (en geç) |

Her tier kendi `subkind` değeriyle DB'de ayrı tutulur:
`iq_d_w1_entry_{bull,bear}`, `iq_d_w2_entry_{bull,bear}`, `iq_d_w3_entry_{bull,bear}`.

## 5. IQ-T Tetik Matrisi (Karar 5 + 6)

**Bullish IQ-D yapısında IQ-T tetikleri:**

| Mevcut dalga | Yön | Bittiğinde IQ-T açılır mı? | IQ-T yönü | Parent ile |
|--------------|-----|----------------------------|-----------|------------|
| W1 oluşuyor | up | hayır | — | — |
| **W2 (düzeltme)** | down | EVET, dipte | **LONG** | aynı yön |
| W3 oluşuyor | up | iç sub-W2 dibinde EVET | LONG | aynı yön |
| **W4 (düzeltme)** | down | EVET, dipte | **LONG** | aynı yön |
| W5 oluşuyor | up | iç sub-W4 dibinde EVET | LONG | aynı yön |
| **A (ABC başı)** | down | EVET, A bitince | **LONG** | karşı yön |
| **B (counter-rally)** | up | EVET, B tepesi | **SHORT** | karşı yön |
| **C (ABC sonu)** | down | EVET, C dibi | **LONG** | aynı yön |

Bearish IQ-D için ayna görüntüsü.

## 6. Yapı Bozulması (Karar 9 + 11)

Standart Elliott invalidation kuralları:

**Bullish IQ-D için:**
1. W2 dibinin altına kırılım (W2 retrace > 100%)
2. W4 W1 zirvesinin altına indi (W4–W1 overlap)
3. W3, W1 ve W5'ten daha kısa oldu (W3 shortest)
4. ABC'de C, W1 başlangıcının altına indi (yapı kompleks korreksiyon olur)
5. ABC sonrası beklenen yeni W1 N bar içinde gelmedi (timeout)
6. Higher-TF regime trend → ranging geçti (opsiyonel — ileride etkin)

**Bearish IQ-D için:** tam ayna kuralları.

**Tetiklendiğinde:**
1. O sembolde TÜM açık IQ-D ve IQ-T pozisyonları **market close**
2. O sembol **lock state**: yeni IQ-D candidate çıkana kadar yeni işlem yok
3. Lock release: yeni IQ-D `iq_d_w1_entry` ya da `iq_d_w2_entry` candidate detect edildiğinde

## 7. TF Kaskat Şeması (Karar 7)

| Parent IQ-D TF | IQ-T TF aralığı | Oran |
|----------------|-----------------|------|
| 4h | 5m, 15m, 30m | 1:8 ila 1:48 |
| 1d | 30m, 1h, 4h | 1:6 ila 1:48 |
| 1w | 4h, 1d | 1:7 ila 1:42 |
| 1M | 1d, 1w | 1:7 ila 1:30 |

1m IQ-T şu aşamada kapsamda yok — ileride 1h IQ-D ile eşlenebilir.

## 8. Sizing Politikası (Karar 10)

```
IQ-D base size  = risk_budget × allocator_size_factor
IQ-T base size  = IQ-D base size × iq_t_size_ratio  // default 0.30
```

Konfig:
- `allocator_v2.iq_t.size_ratio` → 0.30 (default)
- `allocator_v2.iq_d.confluence_min` → 5 (default)
- `allocator_v2.iq_t.confluence_min` → 3 (parent context dahil değil)

> Bu sayılar **veriyle test edilmeden** ön değer; FAZ 25 sonrası backtest
> gridi ile ayarlanacak (FAZ 23 backtest crate'i hazır olduğunda).

## 9. Veri Modeli

```sql
-- qtss_setups extension
ALTER TABLE qtss_setups
  ADD COLUMN parent_setup_id uuid REFERENCES qtss_setups(id),
  ADD COLUMN iq_structure_id uuid;       -- IQ-D structure tracker

-- Yeni profile değerleri:
-- profile IN ('t','d','iq_d','iq_t')

-- Structure tracker (IQ-D yapısı için tek kayıt, ömürlük durum)
CREATE TABLE iq_structures (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    exchange text NOT NULL,
    segment text NOT NULL,
    symbol text NOT NULL,
    timeframe text NOT NULL,
    direction smallint NOT NULL,            -- +1 bull, -1 bear
    state text NOT NULL,                    -- 'tracking', 'completed', 'invalidated', 'locked'
    current_wave text NOT NULL,             -- 'W1','W2','W3','W4','W5','A','B','C'
    structure_anchors jsonb NOT NULL,       -- her dalganın pivot bilgisi
    started_at timestamptz NOT NULL,
    invalidated_at timestamptz,
    invalidation_reason text,
    locked_until_new_iq_d boolean NOT NULL DEFAULT false,
    raw_meta jsonb NOT NULL DEFAULT '{}',
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

-- Symbol lock tablosu (yapı bozulduktan sonra "yeni IQ-D çıkana kadar
-- işlem yok" durumunu tutar)
CREATE TABLE iq_symbol_locks (
    exchange text NOT NULL,
    segment text NOT NULL,
    symbol text NOT NULL,
    locked_at timestamptz NOT NULL DEFAULT now(),
    last_invalidation_id uuid REFERENCES iq_structures(id),
    PRIMARY KEY (exchange, segment, symbol)
);
```

## 10. Implementation Roadmap (PR ayrımı)

| PR | Kapsam | Çıktı |
|----|--------|-------|
| **25A** | nascent / forming / extended impulse detektörlerini engine writer'a bağla — DB'ye yeni `subkind` değerleriyle yaz | DB'de görünür, GUI'de chart üstünde işaretler |
| **25B** | iq_structures + iq_symbol_locks tabloları, structure tracker worker loop | IQ-D yapı durumu canlı izlenir |
| **25C** | IQ-D candidate creator — nascent/forming hit'leri + confluence check + ai gate → qtss_setups'a `profile=iq_d` setup yaz | IQ-D setup'ları açılır |
| **25D** | IQ-T candidate creator — IQ-T tetik matrisini structure context'le birlikte tarayan worker | IQ-T setup'ları açılır |
| **25E** | Allocator profile entegrasyonu — `iq_d` ve `iq_t` profile'larına özgü gate davranışı | Allocator IQ setup'ları execution_bridge'e gönderir |
| **25F** | Invalidation worker — yapı bozulması kuralları + cascade close + symbol lock | Yapı bozulduğunda otomatik kapanış |
| **25G** | GUI — chart'ta IQ-D structure overlay + IQ-T entry markerları | Görsel teyit |

PR-25A şimdi başlıyor.

---

## Veri kaynağı için pivot-based wave bars (Karar 8) — sonraki faz

Mevcut multi-degree ZigZag korunur. Pivot-based wave bars ileri faz (FAZ 25.1)
olarak ayrı geliştirilir; bu tasarımın işleyişi için zorunlu değil — IQ-D
ve IQ-T mantığı LuxAlgo pivots üzerinden de çalışır.
