# FAZ 22 — IQ Setups (Proaktif Kurgu Katmanı)

**Status:** placeholder / spec pending
**Predecessors:** FAZ 11-15 (detectors), FAZ 20 (RADAR), Setup v1.1 (live tick + sanity + cooldown)

## Amaç

Mevcut Setup katmanı (v1.1) **reaktif**: confluence scorer strong_* verdict'i üretince allocator_v2 hemen setup armlar ve dispatch eder. Bu katman kısa-orta vadeli (0.5-2% hedef, saatler) işlemlerde iyi çalışır ama asıl büyük kazançları getiren yapıları yakalamaz:

- **Major dip / major tepe** — haftalık, aylık en düşük / en yüksek
- **Impulse başlangıcı** — Elliott 1. dalga şafakta, daha kimse onaylamadan
- **Impulse sonu** — 5. dalga bitişinde ters dönüşten önce
- **Wyckoff Phase C spring / UTAD** — akümülasyon / dağılımın son oyun teatri

IQ Setups proaktif: bu yapıları **olgunlaşmadan** tespit eder, `state='pending_trigger'` olarak yazar, piyasa kurguya gelince otomatik aktif setup'a döner.

## Yapı 1 vs Yapı 2 karşılaştırma

| | Setup v1.1 | IQ Setup (FAZ 22) |
|---|---|---|
| Tetikleyici | `confluence_snapshots.verdict='strong_*'` | Multi-TF pivot hierarchy + Elliott wave count + Wyckoff phase |
| Zaman penceresi | Son 10 dk (allocator_v2.lookback_minutes) | Son 30 gün (major pivot horizon) |
| Entry timing | Anında (live tick) | Pre-trigger bekler; `entry_zone ± tolerance` fiyata dokununca |
| Hedef | 0.5-2% | 5-30% (impulse full run) |
| Holding | Saatler | Günler-haftalar |
| TP ladder | Ölçülü (ATR × 1.5/3/5) | Fib extension (1.272 / 1.618 / 2.618) + daha büyük |
| Invalidation | Sabit SL | Dinamik (yapısal kırılım: önceki swing low/high ihlali) |

## Bileşenler (detay sonra)

### 1. Major pivot tree
- `qtss_pivots` + L0/L1/L2/L3/L4 hiyerarşi — zaten mevcut
- Multi-degree Elliott pivot graph (docs/memory ref: `project_elliott_multidegree_spec`)
- Son N haftanın "major dip/tepe" pivot'ları → IQ tetikleyici

### 2. Impulse detector
- ZigZag + motive/corrective patern (mevcut qtss-elliott)
- 1. dalga "birthing" kriteri:
  - Wyckoff phase C/D geçişi
  - HH/LL break-of-structure (mevcut qtss-smc)
  - Divergence cascade (RSI/MACD/CVD, multi-TF)
- 5. dalga "exhaustion" kriteri:
  - Diverjans (momentum/volume)
  - Wave-5 truncation olasılığı (wave-3 < wave-1 alert)

### 3. Wyckoff Phase C trigger (zaten var)
- qtss-wyckoff: Spring / UTAD event'leri
- IQ Setup bunları entry_trigger olarak kullanır
- VPOC + VAH/VAL ile hedef seviyeler

### 4. Pre-setup state machine (yeni)
- Yeni state: `pending_trigger`
- Kolonlar: `entry_trigger_price`, `trigger_tolerance_pct`, `trigger_deadline`,
  `invalidation_rules JSONB` (yapısal SL koşulları)
- Yeni loop: `iq_trigger_watcher_loop` — her 2 saniyede pending setup'ları tara,
  live tick ile trigger zone kontrolü + revalidation (confluence hala aynı mı?)

### 5. Confluence bonus (cross-TF)
- Bir sembolün birden fazla TF'de aynı yönde IQ Setup'ı varsa, her biri için
  `confidence *= (1 + 0.15 × other_tf_count)` — multi-TF onay ödülü
- Fair aggregation — her setup ayrı risk, ama bilgi birleşik

### 6. Risk sizing
- IQ Setup notional = Setup v1.1 notional × 2 (büyük kazanç → daha büyük pozisyon)
- Tek anda aktif IQ Setup sayısı üst sınırı: 3 (korelasyon riski)
- `max_concurrent_iq_setups` config

## Veri kaynakları (hepsi mevcut)

| Kaynak | Tablo | Amaç |
|---|---|---|
| Pivot graph | `pivots` (L0-L4) | Major dip/tepe hiyerarşisi |
| Elliott degree | `detections WHERE pattern_family IN ('motive','abc')` | Impulse / correction yapısı |
| Wyckoff phase | `detections WHERE pattern_family='wyckoff'` + phase raw_meta | Spring / UTAD / Phase C-D |
| SMC BOS/CHoCH | `detections WHERE pattern_family='smc'` | Yapısal kırılım |
| Divergence | `indicator_snapshots` + qtss-indicators divergence detector | Momentum exhaustion |
| Volume profile | `qtss-vprofile` output | VPOC/VAH/VAL magnet seviyeleri |

## Implementation iteratif

1. **Faz 22A** — IQ Setup DB şeması + entry_trigger_price + iq_trigger_watcher_loop iskelet
2. **Faz 22B** — Major pivot tree + impulse-birth detector (Elliott wave-1 + Wyckoff Phase C)
3. **Faz 22C** — Impulse-exhaustion detector (wave-5 + divergence cascade)
4. **Faz 22D** — GUI: IQ Setup drawer (farklı renk/ikon) + chart overlay (trigger zone + deadline)
5. **Faz 22E** — Backtest harness: 30 gün + 90 gün retrospective, hit rate ölçümü
6. **Faz 22F** — Production gate: IQ Setup otonom dry modda, kazanç eğrisi Setup v1.1'i geçerse live'a taşı

## Açık sorular (tasarım öncesi yanıtlanmalı)

- IQ Setup deadline: saatler mi (4-12h) yoksa günler mi (3-7 gün)?
- Trigger retrace koşulu: detection pivot'una %X retrace mi, yoksa yapısal tanımlı zone mı (önceki pivot + volume node)?
- Revalidation cadence: her tick'te mi yoksa her bar close'da mı confluence tekrar sorgulansın?
- Position management: IQ Setup'larda trailing stop by default mı (impulse süresi boyunca protect) yoksa sabit SL + scale-out mı?
- AI multi-gate: IQ Setup için farklı eşikler mi kullanılacak (daha yüksek confidence şartı)?

## İlk check-in'de ne istiyoruz

- 1 sembolde 1 IQ Setup `pending_trigger` yaratabiliyoruz
- Trigger zone'u chart'ta görsel olarak gösterebiliyoruz
- Live tick trigger zone'a dokunduğunda setup `armed`'e dönüyor ve normal execution_bridge ile dispatch ediliyor
- RADAR raporunda "IQ setup" ayrı bir sütun olarak görünüyor (v1.1'den ayrışıyor)

---

**İlk iterasyon hedefi:** Faz 22A (iskelet) — tahmini 2-3 gün yoğun iş.

Detaylı tasarım kararları Faz 22 açılışında karar defterine geçer.
