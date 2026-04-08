# QTSS — Project Rules for Claude

Bu dosya her oturumda otomatik yüklenir. Kurallar projenin tamamı için bağlayıcıdır.

## Kodlama Kuralları

### 1. if/else minimizasyonu — fonksiyonlara böl
Kod tabanında dağınık `if/else` ve `match` zincirleri **kullanılmaz**. Bunun yerine:
- **Strategy / dispatch tablosu**: `HashMap<Key, Box<dyn Fn>>` veya `match` yerine trait + impl
- **Polymorphism**: ortak davranışları trait'e taşı, her case kendi `impl` bloğunda
- **Guard / early return**: derin nested if yerine erken `return Err(...)`
- **Look-up**: sabit eşlemeler için config tablosu veya `phf`/`once_cell` map
- **Pattern matching**: zorunluysa `match` kullan ama her kol **tek satır** olsun, mantık fonksiyona delege edilsin

**Why:** Dağınık if/else mantığı zamanla okunamaz hale geliyor, test edilemiyor, yeni venue/strateji eklendiğinde her yerde değişiklik gerekiyor. Fonksiyon/trait ayrımı = açık-kapalı prensibi.

**How to apply:** Yeni kod yazarken önce "bu bir polimorfizm mi, look-up mi, guard mi?" diye sor. 3+ koldan oluşan if/else gördüğünde refactor et. PR review'da çapraz kontrol.

### 2. Tüm değişkenler config tablosunda — kod seviyesinde sabit yok
**Hardcoded sabit yasaktır.** Eşikler, çarpanlar, timeframe listeleri, risk limitleri, API endpoint'leri, retry sayıları, timeout'lar — hepsi PostgreSQL `system_config` (veya yeni `qtss_config`) tablosundan okunur.

- Crate'ler config'i `qtss-config` üzerinden tipli olarak okur (`ConfigKey` enum'u + `get<T>(key)`)
- Default değerler migration ile seed edilir
- Web GUI'de **Config Editor** sayfasından canlı değiştirilebilir
- Değişiklik audit log'a yazılır
- `.env` sadece bootstrap (DB connection) için; iş mantığı değişkenleri orada durmaz

**Why:** Parametre tuning için deploy gerekmemeli; canlıda ayarlanabilmeli; değişiklik geçmişi tutulmalı.

**How to apply:** Yeni bir sayı/string yazacaksan önce "bu config'e gitmeli mi?" diye sor. Cevap büyük ihtimalle evet. İstisna: matematiksel sabitler (π, ATR formülündeki periyot bile config'e girer).

### 3. Detector / Strategy / Adapter ayrımı bozulmaz
- **Detector** sadece "gördüm" der (Detection döner). Validator/target/risk bilmez.
- **Strategy** SignalEnvelope → TradeIntent. Broker bilmez.
- **Risk** TradeIntent → ApprovedIntent. Detector bilmez.
- **Execution Adapter** OrderRequest → broker. Pattern/strateji bilmez.
- **Source Adapter** venue → Bar/Trade. Analiz mantığı bilmez.

Katmanlar arası direkt fonksiyon çağrısı yok — **event bus** üzerinden konuşulur.

### 4. Asset-class agnostic çekirdek
`qtss-elliott`, `qtss-harmonic`, `qtss-classical`, `qtss-wyckoff`, `qtss-range`, `qtss-pivots`, `qtss-regime`, `qtss-validator`, `qtss-target-engine`, `qtss-simulator` crate'leri **venue/asset-class bilmez**. Sadece `Bar`, `PivotTree`, `Indicators`, `RegimeSnapshot` görür. Borsa-özel davranış adapter'lara kapsüllenir.

### 5. Üç çalışma modu
- **live**: gerçek veri + gerçek emir + DB
- **dry**: gerçek veri + simüle emir + DB (ayrı `dry_*` tablolar veya `mode` kolonu)
- **backtest**: tarihsel veri + simüle emir + ayrı backtest tabloları

Mod feature flag değil, **runtime context** — her worker/strategy başlangıçta hangi modda olduğunu bilir.

### 6. İletişim dili: Türkçe
Kullanıcı ile Türkçe konuş. Kod/yorum/commit İngilizce.

## Mimari Referans
Detaylı plan: `docs/QTSS_V2_ARCHITECTURE_PLAN.md`
