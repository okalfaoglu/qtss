# Trendoscope Pine v6 — referans kaynaklar (QTSS hizası)

Bu klasör, **Auto Chart Patterns [Trendoscope®]** göstergesi ve bağımlı kütüphanelerin, projede **Rust/Web** tarafıyla (`crates/qtss-chart-patterns`, `docs/CHART_PATTERNS_TRENDOSCOPE_PORT.md`) karşılaştırma ve golden test için saklanan **Pine Script** kopyalarıdır.

## Lisans

- Çoğu dosya: **CC BY-NC-SA 4.0** — © Trendoscope Pty Ltd. Ticari kullanım için lisansı kendi hukuk danışmanınızla değerlendirin.
- `04_LineWrapper.pine`: Orijinal yayında **Mozilla Public License 2.0** başlığı taşıyor; bu depoda **kaynak metin aynen** korunmuştur.

## Bu sürümde yapılan düzeltmeler

| Konu | Düzeltme |
|------|-----------|
| Gösterge | `divergineTriangleLastPivotDirection` → **`divergingTriangleLastPivotDirection`** (Pine derleyici / okunabilirlik). |
| Import sürümleri | TradingView’da yayınlanan kütüphane sürüm numaraları (`/1`, `/2`, …) **hesabınızdaki** Trendoscope kütüphaneleriyle aynı olmalıdır; farklıysa import satırlarını güncelleyin. |
| `ZigzagLite` | `import Trendoscope/arrays/2` — **Trendoscope `arrays` kütüphanesi** TradingView’da ayrıca yayınlanmış olmalıdır. |

## Dosya listesi

| Dosya | Pine `library()` / gösterge adı |
|-------|----------------------------------|
| `01_indicator_auto_chart_patterns.pine` | `indicator("Auto Chart Patterns …")` |
| `02_utils.pine` | `library('utils')` |
| `03_ohlc.pine` | `library('ohlc')` |
| `04_LineWrapper.pine` | `library('LineWrapper')` |
| `05_ZigzagLite.pine` | `library('ZigzagLite')` — **tam kaynak** (`import Trendoscope/arrays/2` gerekir). |
| `06_abstractchartpatterns.pine` | `library('abstractchartpatterns')` |
| `07_basechartpatterns.pine` | `library('basechartpatterns')` |

QTSS motoru bu Pine’ın tam döngüsünü çalıştırmaz; **`channel-six` / zigzag iskelesi** Rust’ta taşınmıştır. Bu kaynaklar **parite ve inceleme** içindir.
