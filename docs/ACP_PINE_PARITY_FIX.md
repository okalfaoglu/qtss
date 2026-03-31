# ACP [Trendoscope®] Pine Parity — Cursor Düzeltme Rehberi

> **Amaç:** Rust `qtss-chart-patterns` crate'indeki ACP kanal tarama motorunu, orijinal Pine Script v6 (`Trendoscope/abstractchartpatterns/9`, `basechartpatterns/8`, `ZigzagLite/3`) ile **birebir aynı davranışa** getirmek.
>
> **Referans Pine dosyaları** (repo `reference/trendoscope_pine_v6/`):
> - `01_indicator_auto_chart_patterns.pine` — Ana gösterge
> - `05_ZigzagLite.pine` — Zigzag hesaplama
> - `06_abstractchartpatterns.pine` — `inspect`, `ScanProperties`, `DrawingProperties`, `SizeFilters`
> - `07_basechartpatterns.pine` — `find`, `resolve`, `resolvePatternName`
>
> **Etkilenen Rust dosyaları:**
> - `crates/qtss-chart-patterns/src/zigzag.rs`
> - `crates/qtss-chart-patterns/src/scan.rs`
> - `crates/qtss-chart-patterns/src/find.rs`
> - `crates/qtss-chart-patterns/src/resolve.rs`
>
> **Etkilenen web dosyaları:**
> - `web/src/lib/acpChartPatternsConfig.ts`
> - `web/src/lib/acpScanWindow.ts`
> - `web/src/lib/patternDrawingBatchOverlay.ts`
> - `web/src/App.tsx` (tarama tetikleme + overlay hizalama)

---

## HATA LİSTESİ — Pine ile Uyumsuzluklar

### BUG-1: `calculate_bar` — `!replaced` koşulu (Rust'a özgü, Pine'da yok)

**Dosya:** `crates/qtss-chart-patterns/src/zigzag.rs`, satır ~251-252

**Mevcut Rust:**
```rust
let allow_opp = !self.flags.new_pivot || force_double;
if allow_opp && !replaced {
    // karşı pivot ekleme
}
```

**Pine (`ZigzagLite/3` `calculate` metodu):**
```pine
if pDir == 1 and candle.pLowBar == 0 or pDir == -1 and candle.pHighBar == 0 
    and (not this.flags.newPivot or forceDoublePivot)
    // karşı pivot ekleme — "replaced" koşulu YOK
```

**Sorun:** Pine'da aynı barda hem mevcut pivotu güncelleme (adım 1) hem de karşı yönde yeni pivot ekleme (adım 2) olabilir. `forceDoublePivot` mekanizması tam olarak bu durumu yönetir. Rust'taki `&& !replaced` fazladan koşulu, bazı durumlarda karşı pivotun eklenmesini engelliyor → **daha az pivot üretiliyor** → formasyon bulunamıyor.

**Düzeltme:**
```rust
// ÖNCE (hatalı):
if allow_opp && !replaced {

// SONRA (Pine ile uyumlu):
if allow_opp {
```

Aynı şekilde overflow (adım 3) koşulundaki `&& !replaced` da kaldırılmalı:
```rust
// ÖNCE (hatalı):
if overflow && !self.flags.new_pivot && !replaced {

// SONRA (Pine ile uyumlu):
if overflow && !self.flags.new_pivot {
```

**Pine referans — `forceDoublePivot` mantığı:**
```pine
// Pine'da forceDoublePivot kontrol ZAMANI — güncelleme (step 1) SONRASINDA:
if this.zigzagPivots.size() > 1
    llastPivot = this.zigzagPivots.get(1)
    forceDoublePivot := pDir == 1 and candle.pLowBar == 0 ? candle.pLow < llastPivot.point.price : 
                        pDir == -1 and candle.pHighBar == 0 ? candle.pHigh > llastPivot.point.price : false
```

**DİKKAT:** Rust'ta `force_double` hesaplaması step 1 **sonrasına** taşınmalı (şu an öyledir — satır 237-249). Ama `allow_opp` hesabında `replaced` yok, sadece `!self.flags.new_pivot || force_double`. Step 1'de güncelleme yapıldıysa `self.flags.new_pivot = true` olur — `force_double` olmadıkça step 2 atlanır. Bu Pine ile aynı. Sorun `!replaced` ekstra koşulunda.

**Test:** `replaced=true` ama `force_double=true` olan senaryoda Pine karşı pivot ekler, Rust eklemez.

---

### BUG-2: `calculate_bar` — `force_double` hesaplaması güncelleme öncesi yapılıyor

**Dosya:** `crates/qtss-chart-patterns/src/zigzag.rs`, satır ~237

**Mevcut Rust akışı:**
```
1. step 1: aynı yön güncelle (replaced = true)
2. force_double hesapla (pivots[1] üzerinden)
3. step 2: karşı pivot (allow_opp && !replaced)  ← BUG-1
```

**Pine akışı:**
```
1. forceDoublePivot hesapla (zigzagPivots.get(1) — güncelleme ÖNCESİ)
2. step 1: aynı yön güncelle → shift + addnewpivot
3. step 2: karşı pivot (not newPivot or forceDoublePivot)
```

**Sorun:** Pine'da `forceDoublePivot` step 1'den **ÖNCE** hesaplanır — yani güncellenmemiş pivot listesinin `[1]` elemanı kullanılır. Rust'ta ise step 1 sonrasında hesaplanıyor — `pivots[1]` güncelleme sonrası farklı pivot olabilir (shift sonrası eski `[2]` artık `[1]`).

**Düzeltme:** `force_double` hesabını step 1'den **ÖNCE** yap:
```rust
// ÖNCE step 1:
let force_double = if self.pivots.len() > 1 {
    let ll = &self.pivots[1];
    if p_dir_before == 1 && p_low_bar == 0 {
        p_low < ll.point.price
    } else if p_dir_before == -1 && p_high_bar == 0 {
        p_high > ll.point.price
    } else {
        false
    }
} else {
    false
};

// SONRA step 1 (aynı yön güncelle):
if (p_dir_before == 1 && p_high_bar == 0) || ... {
    // shift + add_new_pivot
}

// SONRA step 2 (karşı pivot):
let p_dir = Self::last_trend_dir(self.pivots[0].dir);
let allow_opp = !self.flags.new_pivot || force_double;
if allow_opp {
    // karşı pivot ekleme — !replaced koşulu YOK
}
```

---

### BUG-3: `trend_line_inspect` — Sparse iteration vs Pine dense iteration

**Dosya:** `crates/qtss-chart-patterns/src/scan.rs`, satır ~80-110

**Mevcut Rust:**
```rust
for (&b, ohlc) in bars.range(starting_bar..=ending_bar) {
    total += 1.0;
    // ...
}
```

**Pine (`abstractchartpatterns/9` `inspect` metodu):**
```pine
for barIndex = stratingBar to endingBar by 1
    total := total + 1
    barOhlc = na(ohlcArray) ? ... : ohlcArray.size() <= barIndex ? ... : ohlcArray.get(barIndex)
```

**Sorun:** Pine **her bar_index'i** sırayla ziyaret eder (dense loop). Rust `BTreeMap.range()` kullanıyor — eğer haritada boşluk varsa (atlanan bar_index) o barlar **atlanır**. Bu `total` sayısını etkiler → `score/total` oranını etkiler → inspect geçerlilik kararını etkiler.

**Normal kullanımda** istemci ardışık bar_index gönderdiğinden sorun olmaz. Ama garanti edilmeli.

**Düzeltme:** `trend_line_inspect` fonksiyonunda Pine ile aynı dense loop kullanılmalı:
```rust
pub fn trend_line_inspect(
    p1: (i64, f64),
    p2: (i64, f64),
    starting_bar: i64,
    ending_bar: i64,
    other_bar: i64,
    direction: f64,
    bars: &BTreeMap<i64, OhlcBar>,
    score_ratio_max: f64,
) -> (bool, f64) {
    if starting_bar > ending_bar {
        return (false, 0.0);
    }
    let mut score = 0.0_f64;
    let mut total = 0.0_f64;
    let mut loop_valid = true;
    
    // Pine ile aynı: starting_bar..=ending_bar tüm indeksleri ziyaret et
    for b in starting_bar..=ending_bar {
        let Some(ohlc) = bars.get(&b) else {
            // Pine'da ohlcArray.get(barIndex) — bar yoksa open[bar_index-barIndex] ile erişir
            // Bizim BTreeMap'te bar yoksa atla (Pine'da her zaman var)
            // AMA total'e ekle — Pine toplam bar sayısını kullanır
            total += 1.0;
            continue;
        };
        total += 1.0;
        // ... mevcut inspect mantığı aynen devam eder
    }
    // ...
}
```

**Alternatif (daha güvenli):** İstemciden gelen bar dizisinin ardışık olduğunu garanti et. `channel_six_scan` API handler'da: gelen `bars` listesinde `bar_index` 0..n-1 ardışık olmalı. Zaten `chartOhlcRowsToScanBars` bunu yapıyor (`bar_index: j` ile sıralı). Bu durumda `BTreeMap.range()` zaten dense iteration yapar. **Ama doğrulama ekle:**

```rust
// channel_six_scan API handler'da:
let expected_count = (ending_bar - starting_bar + 1) as usize;
let actual_count = bars.range(starting_bar..=ending_bar).count();
if expected_count != actual_count {
    tracing::warn!(expected_count, actual_count, "bar_index gaps detected — inspect score may differ from Pine");
}
```

---

### BUG-4: `resolve_pattern_type_id` — `>=` vs Pine `>`

**Dosya:** `crates/qtss-chart-patterns/src/resolve.rs`, satır ~38

**Mevcut Rust:**
```rust
let t1_left_higher_or_equal = t1p1 >= t2p1; // ← Rust: >=
```

**Pine (`basechartpatterns/8` `resolvePatternName`):**
```pine
upperAngle = t1p1 > t2p1 ? ... : ... // ← Pine: >
```

**Sorun:** `t1p1 == t2p1` durumunda Pine `else` dalına girer (çoğunlukla 0 döndürür, geçersiz). Rust `if` dalına girer ve farklı pattern id üretebilir. Bu genelde "iyileştirme" olarak görülebilir ama Pine parity hedefinde **fark yaratır**.

**Düzeltme:** Pine ile birebir uyum için `>` kullanılmalı:
```rust
let t1_left_higher = t1p1 > t2p1; // Pine ile aynı
```

Ancak bu durumda `t1p1 == t2p1` olan geçerli formasyonlar kaybolur. **İki aşamalı yaklaşım:**
1. İlk olarak Pine ile birebir uyumu sağla (`>`)
2. Sonra opsiyonel "enhanced mode" olarak `>=` flag'i ekle

---

### BUG-5: `next_level_from_zigzag` — Pine `nextlevel` ile tam uyumsuzluk

**Dosya:** `crates/qtss-chart-patterns/src/zigzag.rs`, fonksiyon `next_level_from_zigzag`

**Mevcut Rust — sadeleştirilmiş yaklaşım:**
```rust
// Yalnızca |dir|==2 pivotları al, aynı yöndekileri sıkıştır
for p in chrono {
    if p.dir.abs() != 2 { continue; }
    // compress same-sign
}
```

**Pine (`ZigzagLite/3` `nextlevel` metodu):**
```pine
method nextlevel(Zigzag this) =>
    nextLevel = Zigzag.new(this.length, this.numberOfPivots, 0)
    // Ters sırada döner (eski → yeni)
    for i = this.zigzagPivots.size() - 1 to 0 by 1
        lPivot = Pivot.copy(this.zigzagPivots.get(i))
        dir = lPivot.dir
        newDir = math.sign(dir)
        if nextLevel.zigzagPivots.size() > 0
            lastPivot = nextLevel.zigzagPivots.get(0)
            lastDir = math.sign(lastPivot.dir)
            if math.abs(dir) == 2
                if lastDir == newDir
                    if dir * lastValue < dir * value
                        nextLevel.zigzagPivots.shift()  // eskiyi kaldır, yenisi daha iyi
                    else
                        // tempPivot ile ara geçiş dene
                else
                    // tempFirst + tempSecond geçiş kontrolleri
                nextLevel.addnewpivot(lPivot)  // eklenir
                tempBullishPivot := na
                tempBearishPivot := na
            else
                // |dir|==1: tempBullishPivot/tempBearishPivot'a kaydet
```

**Sorun:** Pine'ın `nextlevel` mantığı **çok daha karmaşık** — `tempBullishPivot`/`tempBearishPivot` geçici tamponlarıyla `|dir|==1` pivotları da belirli koşullarda üst seviyeye taşır. Rust sadece `|dir|==2` pivotları alıp sıkıştırıyor. Bu, üst seviye zigzagda **daha az pivot** üretilmesine yol açıyor → daha az formasyon bulunuyor.

**Düzeltme:** Pine'daki `nextlevel` mantığı tam olarak taşınmalı. Aşağıdaki pseudocode Rust'a çevrilmeli:

```rust
pub fn next_level_from_zigzag(source: &ZigzagLite) -> ZigzagLite {
    let mut next = ZigzagLite::new(source.length, source.number_of_pivots, 0);
    next.level = source.level + 1;
    
    let chrono = pivots_chronological(source); // eski → yeni
    let mut temp_bullish: Option<ZigzagPivot> = None;
    let mut temp_bearish: Option<ZigzagPivot> = None;
    
    for p_ref in chrono {
        let mut lp = p_ref.clone();
        let dir = lp.dir;
        let new_dir = dir.signum();
        let value = lp.point.price;
        lp.level += 1;
        
        if next.pivots.is_empty() {
            if dir.abs() == 2 {
                next.add_new_pivot(lp);
            } else {
                // |dir|==1: temp'e kaydet
                if new_dir > 0 { temp_bullish = Some(lp); }
                else { temp_bearish = Some(lp); }
            }
            continue;
        }
        
        let last_dir = next.pivots[0].dir.signum();
        let last_value = next.pivots[0].point.price;
        
        if dir.abs() == 2 {
            if last_dir == new_dir {
                // Aynı yönde: daha iyi olanı tut
                if (dir as f64) * last_value < (dir as f64) * value {
                    next.pivots.remove(0); // eskiyi kaldır
                } else {
                    // tempPivot ile geçiş dene
                    let temp = if new_dir > 0 { &temp_bearish } else { &temp_bullish };
                    if let Some(tp) = temp {
                        next.add_new_pivot(tp.clone());
                    } else {
                        continue;
                    }
                }
            } else {
                // Farklı yönde: tempFirst+tempSecond geçiş kontrolü
                let temp_first = if new_dir > 0 { &temp_bullish } else { &temp_bearish };
                let temp_second = if new_dir > 0 { &temp_bearish } else { &temp_bullish };
                if let (Some(tf), Some(ts)) = (temp_first, temp_second) {
                    let temp_val = tf.point.price;
                    if (new_dir as f64) * temp_val > (new_dir as f64) * value {
                        next.add_new_pivot(tf.clone());
                        next.add_new_pivot(ts.clone());
                    }
                }
            }
            next.add_new_pivot(lp);
            temp_bullish = None;
            temp_bearish = None;
        } else {
            // |dir|==1: temp'e kaydet (daha iyi olanı tut)
            let temp = if new_dir > 0 { &mut temp_bullish } else { &mut temp_bearish };
            if let Some(existing) = temp {
                if (dir as f64) * value > (dir as f64) * existing.point.price {
                    *temp = Some(lp);
                }
            } else {
                *temp = Some(lp);
            }
        }
    }
    
    // Pine güvenlik: üst seviye alt seviyeden yoğunsa temizle
    if next.pivots.len() >= source.pivots.len() {
        next.pivots.clear();
    }
    next
}
```

---

### BUG-6: `inspect_pick_best_three_point_line` — Pine'daki seçim mantığı

**Dosya:** `crates/qtss-chart-patterns/src/scan.rs`, satır ~118-160

**Mevcut Rust:**
```rust
let pick = if ok1 && s1 > s2.max(s3) { 1 }
    else if ok2 && s2 > s1.max(s3) { 2 }
    else { 3 };
```

**Pine (`abstractchartpatterns/9`):**
```pine
returnItem = valid1 and score1 > math.max(score2, score3) ? 1 : 
             valid2 and score2 > math.max(score1, score3) ? 2 : 3
```

**Sorun:** Pine'da `returnItem=3` seçildiğinde `[valid3, trendLine3]` döner. Rust'ta da aynı. **UYUMLU** ama ince fark: tüm `ok1/ok2/ok3` false olduğunda Pine `returnItem=3` döner ve `valid3=false` + `trendLine3` döner. Rust'ta da `pick=3` + `final_ok=ok3`. Bu durumda caller'da `if !upper_ok` yakalanır. ✅

Ama `ok1=true, ok2=true, ok3=true` ve `s1==s2>s3` durumunda Pine `returnItem=1` döner (ilk koşul `s1 > max(s2,s3)` — `s1 == s2` ise `s1 > s2` false → `returnItem=2` kontrol → `s2 > max(s1,s3)` — `s2==s1` ise false → `returnItem=3`). Rust'ta da aynı — `s1 > s2.max(s3)` → `s1 > s2` false → pick 2 kontrol → false → pick=3. **UYUMLU**. ✅

---

### BUG-7: `analyze_channel_six_from_bars` — Pine `while` vs Rust `for` seviye döngüsü

**Dosya:** `crates/qtss-chart-patterns/src/find.rs`, satır ~890

**Pine:**
```pine
while(mlzigzag.zigzagPivots.size() >= 6+offset)
    // find()
    if valid → break
    mlzigzag := mlzigzag.nextlevel()
```

**Rust:**
```rust
'levels: for _ in 0..=max_zigzag_levels {
    // scan
    zz = next_level_from_zigzag(&zz);
    if zz.pivots.len() < scan_params.number_of_pivots { break; }
}
```

**Sorunlar:**
1. Pine **sınırsız** seviye döngüsü yapar (pivotlar yeterli olduğu sürece). Rust `max_zigzag_levels` (varsayılan 2) ile sınırlı.
2. Pine her zigzag config için **ilk geçerli bulduğunda** (`valid`) o config için döngüyü keser. Rust tüm seviyeleri tarayıp `max_matches`'e kadar toplar. Bu farklılık çok formasyon döndürür ama Pine'ın "en güncel formasyonu bul" davranışından sapma.

**Düzeltme — Pine parity modu:**
```rust
// max_zigzag_levels varsayılanı: 0 = sınırsız (Pine davranışı)
// Her zigzag config için: ilk geçerli formasyonda o config'in seviye döngüsünü kes
// max_matches=1 ise Pine ile birebir aynı davranış
```

Pratik olarak `max_zigzag_levels` varsayılanını **0** (sınırsız, pivot yeterliliğine kadar) yapmak ve `max_matches=1` varsayılanını korumak.

---

### BUG-8: `pivot_candle` — `>=` vs Pine `ta.highest` davranışı

**Dosya:** `crates/qtss-chart-patterns/src/zigzag.rs`, satır ~146

**Mevcut Rust:**
```rust
if h >= p_high {  // eşit → güncelle (son gelen kazanır)
    p_high = h;
    p_high_back = (i - j) as i32;
}
if l <= p_low {   // eşit → güncelle (son gelen kazanır)
    p_low = l;
    p_low_back = (i - j) as i32;
}
```

**Pine `ta.highest` / `ta.highestbars`:**
Pine'ın built-in `ta.highest(high, length)` birden fazla eşit tepe varsa **en eski** olanı döndürür (bar_index en büyük = en geride). `ta.highestbars` negatif geri sayım döndürür.

**Sorun:** Rust `>=` ile son gelen (en yeni bar) kazanır. Pine en eski eşiti kazandırır. Bu `p_high_bar` / `p_low_bar` değerini etkiler → pivot zamanlamasını etkiler.

**Düzeltme:** Eşit değerlerde **ilk bulunanı** (yani eski barı) korumak için `>` ve `<` kullanılmalı:
```rust
if h > p_high {   // kesin büyük — Pine: ta.highest ile aynı
    p_high = h;
    p_high_back = (i - j) as i32;
}
if l < p_low {    // kesin küçük
    p_low = l;
    p_low_back = (i - j) as i32;
}
```

**Not:** İlk iterasyonda `p_high = NEG_INFINITY` ve `p_low = INFINITY` olduğundan ilk bar her zaman `>` / `<` ile yakalanır. Sonraki eşitlerde eski kalır — Pine davranışı.

---

### BUG-9: Web overlay — `bar_index` hizalama yarışı

**Dosya:** `web/src/App.tsx`, satır ~1010-1031

**Mevcut kod:**
```ts
const multiOverlay = useMemo(() => {
    if (!lastChannelScan?.matched || !bars?.length) return null;
    const scanWindow = acpOhlcWindowForScan(bars, acpConfig.calculated_bars, acpConfig.scanning.repaint);
    const scanLen = Math.min(lastChannelScan.bar_count, scanWindow.length);
    const scanBars = scanLen > 0 ? scanWindow.slice(-scanLen) : scanWindow;
    const fromMatches = buildMultiPatternOverlayFromScan(lastChannelScan, scanBars, acpConfig.display);
    // ...
}, [lastChannelScan, bars, ...]);
```

**Sorun:** `bars` state'i live poll ile sürekli güncelleniyor. Tarama anındaki bars ile overlay render anındaki bars farklı uzunlukta olabilir → `bar_index` hizalaması bozulur → çizim noktaları `null` döner → formasyon grafikte **görünmez**.

**Düzeltme:** Tarama sonucuyla birlikte o anki bars snapshot'ını kaydet:
```ts
// App.tsx'te yeni state:
const [scanBarsSnapshot, setScanBarsSnapshot] = useState<ChartOhlcRow[] | null>(null);

// runChannelSixScanWithBars içinde:
const scanWindow = acpOhlcWindowForScan(barsInput, ...);
const payload = chartOhlcRowsToScanBars(scanWindow);
const res = await scanChannelSix(token, { bars: payload, ... });
setLastChannelScan(res);
setScanBarsSnapshot(scanWindow); // ← tarama anındaki bars'ı kaydet

// multiOverlay useMemo'da:
const multiOverlay = useMemo(() => {
    if (!lastChannelScan?.matched || !scanBarsSnapshot?.length) return null;
    const scanLen = Math.min(lastChannelScan.bar_count, scanBarsSnapshot.length);
    const scanBars = scanLen > 0 ? scanBarsSnapshot.slice(-scanLen) : scanBarsSnapshot;
    return buildMultiPatternOverlayFromScan(lastChannelScan, scanBars, acpConfig.display);
}, [lastChannelScan, scanBarsSnapshot, acpConfig.display]);
```

---

### BUG-10: Varsayılan `depth: 55` — pivot kıtlığı

**Dosya:** `web/src/lib/acpChartPatternsConfig.ts`, satır ~126

**Pine varsayılanı:** `depth1 = input.int(55, ...)` — Pine'da `depth` zigzag'ın `numberOfPivots` (max pivot sayısı) parametresi.

**Mevcut Rust:** `zigzag_from_ohlc_bars(bars, zigzag_length, max_pivots, ...)` — burada `max_pivots = depth = 55`. Bu, zigzag'ın **en fazla 55 pivot tutmasını** sağlar. 55 pivot yeterli.

**AMA:** `ZigzagLite::new(length, number_of_pivots, offset)` — `number_of_pivots = max_pivots = depth`. Pine'da `Zigzag.new(length, depth, 0)` aynı şey. **SORUN BURADA DEĞİL.**

Pine'da `depth` = max pivot limiti (varsayılan 55, yeterli). `length` = pivot pencere boyutu (varsayılan 8). `length=8` ve yeterli mum varsa Pine ~100+ pivot üretir. Rust da üretmeli.

**Gerçek sorun:** BUG-1 ve BUG-2 nedeniyle Rust daha az pivot üretiyor. Zigzag parametreleri Pine ile aynı ama pivot üretim mantığı farklı → daha az pivot → 6 alterne pivot bulunamıyor.

---

## DÜZELTME SIRASI

| # | Öncelik | Dosya | Düzeltme | Test |
|---|---------|-------|----------|------|
| 1 | **KRİTİK** | `zigzag.rs` | BUG-1: `!replaced` koşulunu kaldır | Aynı OHLC seti ile Pine ve Rust pivot sayısını karşılaştır |
| 2 | **KRİTİK** | `zigzag.rs` | BUG-2: `force_double` hesabını step 1'den önceye taşı | Double pivot senaryosu testi |
| 3 | **KRİTİK** | `zigzag.rs` | BUG-8: `pivot_candle` `>=`/`<=` → `>`/`<` | Eşit fiyatlı mumlarla test |
| 4 | **KRİTİK** | `zigzag.rs` | BUG-5: `next_level_from_zigzag` tam Pine taşıması | Multi-level zigzag pivot karşılaştırması |
| 5 | **YÜKSEK** | `resolve.rs` | BUG-4: `>=` → `>` (Pine parity) | Eşit sol uçlu pattern testi |
| 6 | **YÜKSEK** | `scan.rs` | BUG-3: Dense iteration veya doğrulama | Gap'li bar dizisi ile score/total testi |
| 7 | **YÜKSEK** | `find.rs` | BUG-7: `max_zigzag_levels=0` (sınırsız) varsayılan | Pine ile aynı formasyonu bulmayı doğrula |
| 8 | **ORTA** | `App.tsx` | BUG-9: `scanBarsSnapshot` state | Live poll + scan yarışı senaryosu |

---

## TEST STRATEJİSİ

### Golden Test: Pine vs Rust Pivot Eşitliği

1. TradingView'da ACP göstergesini BTCUSDT 1h'ye uygula, `length=8, depth=55` ile
2. Pine `zigzag.zigzagPivots` listesini logla (bar_index, price, dir)
3. Aynı OHLC verisini Rust `zigzag_from_ohlc_bars` ile işle
4. Pivot sayısı ve konumları **birebir aynı** olmalı

Bu test BUG-1, BUG-2, BUG-8'i doğrular. Pivotlar eşleştiğinde formasyon bulma otomatik olarak düzelir.

### Birim Testleri (her BUG için)

**BUG-1 testi:**
```rust
#[test]
fn double_pivot_not_blocked_by_replaced() {
    // Senaryo: bar i'de tepe güncellenir (replaced=true) VE aynı barda
    // dip, llastPivot'un altına iner (forceDouble=true)
    // Pine: karşı pivot eklenir
    // Eski Rust: !replaced → engellenir
    // Yeni Rust: eklenir
}
```

**BUG-5 testi:**
```rust
#[test]
fn next_level_includes_dir1_pivots_via_temp_buffer() {
    // |dir|==1 pivotları temp'e alınıp belirli koşulda üst seviyeye taşınmalı
    // Eski Rust: sadece |dir|==2 alır
    // Yeni Rust: Pine ile aynı
}
```

**BUG-8 testi:**
```rust
#[test]
fn pivot_candle_equal_highs_keeps_oldest() {
    let h = [5.0, 5.0, 5.0, 3.0]; // üç eşit tepe
    let l = [1.0, 1.0, 1.0, 1.0];
    let (phb, _, ph, _) = ZigzagLite::pivot_candle(4, 3, &h, &l);
    // Pine: ta.highestbars → en eski eşiti döndürür → phb = 3 (3 bar geri)
    assert_eq!(phb, 3);
    assert!((ph - 5.0).abs() < 1e-9);
}
```

### Entegrasyon Testi

`crates/qtss-chart-patterns/testdata/pine_parity/` altında:
- `btcusdt_1h_500bars.json` — OHLC verisi
- `btcusdt_1h_pine_pivots.json` — Pine'dan alınan pivot listesi
- `btcusdt_1h_pine_patterns.json` — Pine'dan alınan formasyon sonuçları

```rust
#[test]
fn pine_parity_btcusdt_1h() {
    let bars = load_test_bars("btcusdt_1h_500bars.json");
    let pine_pivots = load_pine_pivots("btcusdt_1h_pine_pivots.json");
    let zz = zigzag_from_ohlc_bars(&bars, 8, 55, 0);
    let rust_pivots = pivots_chronological(&zz);
    assert_eq!(rust_pivots.len(), pine_pivots.len());
    for (rp, pp) in rust_pivots.iter().zip(pine_pivots.iter()) {
        assert_eq!(rp.point.index, pp.bar_index);
        assert!((rp.point.price - pp.price).abs() < 1e-8);
        assert_eq!(rp.dir.signum(), pp.dir.signum());
    }
}
```

---

## ÖZET: Neden Formasyon Görünmüyor?

Ana neden **BUG-1 + BUG-2 + BUG-8 kombinasyonu**: Zigzag daha az pivot üretiyor → 6 alterne pivot bulunamıyor → tarama `InsufficientPivots` reject kodu ile sonuçlanıyor → grafikte formasyon yok.

BUG-1'i düzeltmek tek başına %70 ihtimalle sorunu çözer. Geri kalan BUG'lar Pine ile tam eşitlik için gereklidir.

---

*Bu doküman `docs/` dizinine yerleştirilmemelidir — yalnızca Cursor'ın düzeltme referansıdır. Düzeltmeler tamamlandıktan sonra atılabilir.*
