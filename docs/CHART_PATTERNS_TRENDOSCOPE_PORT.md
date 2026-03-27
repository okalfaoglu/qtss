# Trendoscope «Auto Chart Patterns» (ACP) → QTSS taşıma notları

Kaynak: TradingView Pine v6 göstergesi ve kütüphaneleri (Trendoscope®). Orijinal lisans: **CC BY-NC-SA 4.0** — ticari kullanım için hukuk değerlendirmesi önerilir. Bu depoda `qtss-chart-patterns`: geometri / `inspect` / `resolve`, **zigzag iskelesi**, 6 pivotlu **kanal taraması** ve **çizim JSON** (`PatternDrawingBatch`); tam ACP göstergesi döngüsü (scanner, çoklu katman, `allowedPatterns`) henüz tamamlanmadı.

## 0. Paylaşılan Pine — modül analizi (1-1 eşleme hedefi olan parçalar)

| Modül | Rol | QTSS’te 1-1 port zorluğu |
|--------|-----|---------------------------|
| **indicator ACP** | Girdi, `Scanner`, çoklu zigzag, `getZigzagAndPattern` → `find` → `draw`, `maxPatterns`, repaint | QTSS: çoklu zigzag birleştirme + `lastPivotDirection`/`custom` + `while zz≥6` hizası eklendi; repaint stateless API’de yok |
| **utils** | Tema renkleri, zaman farkı, `check_overflow`, `get_trend_series` | Orta (genel matematik / UI) |
| **ohlc** | `OHLC` tipi, dizi push/limit | Düşük |
| **LineWrapper** | `Line`, `draw`, `get_price` | `get_price` → `qtss_chart_patterns::line_price_at_bar_index` |
| **ZigzagLite** | Pivot, `calculate`, `nextlevel`, çok seviye pivot ağacı, bayraklar | **Çok yüksek** (çekirdek durum makinesi) |
| **abstractchartpatterns** | `Pattern`, `ScanProperties`, `DrawingProperties`, `inspect`, `checkBarRatio`, `checkSize`, `ignoreIfEntryCrossed`, `draw`/`erase`/`push` | **Çok yüksek**; `checkSize` + `ignoreIfEntryCrossed` sunucu taramasında |
| **basechartpatterns** | `find(points)`, `find(zigzag,…)`, `resolve` (çizgileri pattern uç mumlarına uzatma), `resolvePatternName`, `getPatternNameById` | `resolve_pattern_type_id` artık Pine’daki gibi **uzatılmış** üst/alt çizgi fiyatları + `barDiff = last-first` kullanır |

**Çizim yönetimi (Pine ↔ QTSS):** TradingView `line` / `polyline` / `label` nesne ömrü; QTSS’te `PatternDrawingBatch` + `batch_id` ile grupla, son N batch’i sil (Pine `maxPatterns` + `deleteOnPop`).

## 1. Pine modül grafiği (sizin paylaştığınız import’lar)

```
indicator ACP
  → utils (Theme, getColors, …)
  → ohlc (OHLC, push, …)
  → LineWrapper (Line, draw, get_price)
  → ZigzagLite (Zigzag, calculate, nextlevel, find → base’e köprü)
  → abstractchartpatterns (Pattern, ScanProperties, DrawingProperties, draw, erase, inspect, checkBarRatio)
  → basechartpatterns (find(zigzag,…), getPatternNameById, resolvePatternName)
```

**Çalışma zamanı akışı (özet):**

1. Onaylı bar (`barstate.isconfirmed`) veya `repaint` ile her güncellemede `scanner.scan()`.
2. Etkin zigzag katmanları için `getZigzagAndPattern(length, depth, ohlcArray)`:
   - `zigzag.calculate([high, low])`
   - Yeni pivot (`zigzag.flags.newPivot`) ise, çok seviyeli döngü: `find(sProperties, dProperties, patterns, ohlc)` + `currentPattern.draw()` + `patterns.push(..., maxPatterns)`.

## 2. QTSS’te eşlenen parçalar (`crates/qtss-chart-patterns`)

| Pine | Rust |
|------|------|
| `Line.get_price(bar)` (bar_index uzayı) | `line_price_at_bar_index` |
| `getPatternNameById` | `pattern_name_by_acp_id` |
| `Pattern.draw()` / toplu silme konsepti | `DrawingCommand`, `PatternDrawingBatch` (serde JSON → LWC `LineSeries` / marker) |
| `ZigzagLite` (iskele) | `ZigzagLite::pivot_candle`, `calculate_bar`, `run_series`, `next_level_from_pivot_prices` |
| `find` (6 alterne pivot → iki `inspect` → `resolve`) | `zigzag_from_ohlc_bars`, `scan_six_alternating_pivots`, `try_scan_channel_six_from_bars` |
| Pine `ScanProperties.offset` (en yeni pivotları atlayarak arama) | `analyze_channel_six_from_bars(..., pivot_tail_skip_max)` + `six_pivots_chrono_tail_skip` |
| — | API: `POST /api/v1/analysis/patterns/channel-six` (JWT + dashboard rolleri), gövde: `{ "bars": [OhlcBar, …], "zigzag_length"?, "pivot_tail_skip_max"?, … }` |

## 3. Henüz taşınmayan (yüksek karmaşıklık)

- **ZigzagLite** ile Pine’ın birebir tüm edge-case’leri ve tam **`nextlevel`** ağacı (Rust’ta iskele var; golden test ile sıkılaştırılabilir).
- **`find(zigzag,…)` tam döngüsü** — çoklu aday, `allowedPatterns`, `avoidOverlap`, çoklu zigzag katmanı.
- **Scanner / draw / maxPatterns** — çizim yaşam döngüsü (JSON batch tarafı kısmen var).

Bunlar için öneri: bar başına Pine çıktısı ile **golden test** (örn. küçük OHLC dizisi → beklenen pivot listesi).

## 4. Çizim yönetimi (1-1 yönetim modeli)

TradingView: `line.new`, `polyline.new`, `label.new`, `delete`, `deleteOnPop`.

QTSS: Motor yalnızca **`PatternDrawingBatch`** üretir; frontend:

- Komutları sırayla uygular veya tek `layer` grubu olarak tutar.
- Silme: batch `id` + “son N formasyon” politikası (Pine `maxPatterns`).

## 5. Web (qtss-web)

### 5.1 Grafik verisi (TradingView benzeri sembol / zaman dilimi)

Üst çubukta **sembol**, **interval** (örn. `1m` … `1d`), **bar sayısı** (100 / 250 / 500 / 1000; API tarafında üst sınır ayrıca 5000’e kadar), **Yükle** ve **Sığdır** (tüm veriyi sığdır) bulunur. Sembol değişimi **~450 ms debounce** ile yüklenir; hata metni üst çubukta kısaltılmış gösterilir (`title` ile tam metin).

| Mod | Kaynak | Canlı (açık) mum |
|-----|--------|-------------------|
| **OAuth + API** | `fetchMarketBarsRecent` → `market_bars` | İsteğe bağlı **otomatik yenile** (45 sn, tam liste yenilenir). |
| **Giriş yok** (`SYMBOLUSDT` formatı) | Binance spot **REST** klines | **WebSocket** `@kline_<interval>`: `mergeBinanceKlineIntoCandles` ile son / yeni mum güncellenir; dizi seçilen **limit** ile kısaltılır. |

**Proxy / CORS:** Geliştirmede Vite `vite.config.ts` içinde `/__binance` → `https://api.binance.com` (path rewrite). Üretim derlemesinde tarayıcı doğrudan Binance’e gidemeyebilir; `VITE_BINANCE_API_BASE` ile aynı origin veya CORS izinli bir taban URL tanımlanmalı (`web/src/api/binancePublic.ts`).

**Kısayollar (odak input’ta değilken):** **R** — barları yenile (giriş varsa API, yoksa Binance REST); **End** — `timeScale.scrollToRealTime()` (`scrollLatestSeq`). **Esc** — çizim aracı / menü. **Shift+F** — tam ekran (bkz. aşağı).

İzleme listesinden sembol seçimi üst alanı günceller; API oturumu varsa ilgili sembol için barlar yeniden çekilir, yoksa debounce sonrası Binance yolu çalışır.

### 5.2 CSV, formasyon çizimi, tam ekran

- Görünen mumlar: **CSV indir** (UTF-8 BOM; sembol + interval + OHLCV) — hamburger menü veya Geliştirici paneli.
- **`PatternDrawingBatch` önizleme**: Geliştirici / API bölümünde JSON → *Formasyonu uygula*:
  - `trend_line`, `zigzag_polyline` → ek `LineSeries`;
  - `pattern_label`, `pivot_label` → mum serisinde `setMarkers` (kare / daire, metin).
  - Zaman: `time_ms > 0` veya `bar_index` ile mevcut mum penceresi.
- **Tam ekran**: önce **Fullscreen API** (`requestFullscreen`); destek yoksa `tv-root--fullscreen` sınıfı. Üst çubuk veya **Shift+F**; **Esc** ile çıkış (CSS modunda uygulama; tarayıcı tam ekranında genelde tarayıcı önce kapanır).

### 5.3 ACP / zigzag ile hizalama

Motor tarafında `scan` ve zigzag girdi olarak **aynı OHLC dizisini** kullanmalı. **Web:** hamburger menüde OAuth sonrası **Kanal taraması (6 pivot)** — görünen mumlar `open_time` ile sıralanıp `bar_index = 0..n-1` olarak `POST /api/v1/analysis/patterns/channel-six` gövdesine gider (`bar_ratio_enabled: false`, `pivot_tail_skip_max: 0` (Pine `ScanProperties.offset`), `max_zigzag_levels: 2` örnek).

`allowedPatterns` eşleniği: istekte `allowed_pattern_ids` gönderilebilir. Boşsa tüm pattern id’ler kabul edilir. Web çekmecesinde **allowed_pattern_ids (1–13, virgül)** alanı bu listeyi doldurur; boş bırakılırsa alan API’ye gönderilmez.

Yeni yanıt alanları:

- `bar_count`, `zigzag_pivot_count`: eşleşme olsa/olmasa teşhis.
- `reject.code`: neden elendi (`insufficient_pivots`, `bar_ratio_*`, `inspect_*`, …).
- `reject.code = pattern_not_allowed`: geometri geçti ama bulunan id, `allowed_pattern_ids` filtresinde yok.
- `outcome.pivot_tail_skip`: eşleşmenin kaç pivot atlayarak bulunduğu (`0` = en güncel 6’lı).
- `outcome.zigzag_level`: eşleşmenin bulunduğu zigzag seviyesi (`0` = temel, `>0` = `nextlevel`).
- `drawing`: üst/alt kanal çizgisi uçları (`bar_index`, `price`) — web’de line overlay olarak çizilir.

Girişsiz Binance modunda bu uç JWT istediği için tarama yalnızca **Giriş dene** sonrası çalışır.

## 6. Sonraki adımlar

1. Zigzag / `find` golden testleri (Pine veya el ile beklenen pivotlar).
2. `avoidOverlap`, çoklu zigzag katmanı (genişletme); `allowedPatterns` → API + web alanı mevcut.
3. İsteğe bağlı: JWT’siz önizleme (proxy veya ayrı rate-limit’li uç).
4. Web: tarama sonucunu `PatternDrawingBatch` / grafik marker’larına bağlama; etiket overlay.
5. API: `market_bars` kimliği ile tarama (sunucu içi OHLC, gövde yerine referans).
