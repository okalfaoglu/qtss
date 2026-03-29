# Data sources — `source_key` naming (PLAN Phase A)

Wire ve veritabanında **`source_key`** değerleri **İngilizce `snake_case`**, kararlı ve tekrarlanabilir olmalıdır. UI metinleri Türkçe kalabilir.

## Kaynak politikası (özet — ayrıntı PLAN §2)

| Durum | Kaynak |
|--------|--------|
| **Projede kullanılmaz** | CryptoQuant **ücretsiz** (günlük gecikme); Whale Alert **ücretsiz** (limit + gecikme) |
| **Birincil gerçek zamanlı** | Binance USDT-M **FAPI** (funding, OI, taker); **Hyperliquid** `info`; **Coinglass** (API key + kota); **Nansen** (smart money, bilinçli tick) |
| **Tamamlayıcı** | **DeFi Llama** — yalnızca **TVL trend** (ör. 30 dk+ tick); DEX alım/satım baskısı için **birincil kaynak değil** |
| **DEX baskısı (hedef)** | Nansen screener alanları + **The Graph** / **DexScreener** / benzeri ( `external_fetch` + `signal_scorer` ) |
| **Balina / büyük transfer (hedef)** | Coinglass uygun uçlar; HL `clearinghouseState` (yol haritası); isteğe bağlı **Arkham** |

## Kurallar

| Kural | Örnek |
|--------|--------|
| Küçük harf, kelimeler `_` | `binance_taker_btcusdt` |
| Borsa / ürün öneki net | `coinglass_netflow_btc`, `nansen_token_screener` |
| Sembol bazlı kaynaklarda quote sabitse sonek | `…_btcusdt` (Binance USDT çifti) |
| Aynı metriği farklı varlıkta kopyalamak için yeni satır | `binance_taker_ethusdt` — `external_data_sources` + ops API veya SQL |

## Bilinen anahtarlar (referans)

| `source_key` | Kaynak | Not |
|--------------|--------|-----|
| `nansen_token_screener` | Worker `nansen_engine` | `data_snapshots` + `nansen_snapshots`; confluence smart-money sütunu |
| `binance_taker_{base}usdt` | `external_data_sources` veya seed | `{base}` = sembolün USDT öncesi kısmı küçük harf; confluence `onchain` |
| `binance_premium_{base}usdt` | Migration `0028_*` (ör. BTC) | `GET /fapi/v1/premiumIndex` — `lastFundingRate` |
| `binance_open_interest_{base}usdt` | Migration `0028_*` | `GET /fapi/v1/openInterest` — OI ısısı |
| `hl_meta_asset_ctxs` | Migration `0026_*` (varsayılan kapalı) | Hyperliquid POST `info` |
| `coinglass_netflow_btc` | Seed / manuel | Coinglass API anahtarı `headers_json` içinde |
| `coinglass_liquidations_btc` | Migration `0028_*` (varsayılan kapalı) | Likidasyon yönü — `signal_scorer` esnek ayrıştırıcı |

**The Graph / DeFi Llama:** Sabit ücretsiz uç yok; subgraph veya REST URL’nizi `external_data_sources` ile ekleyip ham JSON’u `data_snapshots`’a alın. Özel şema için `crates/qtss-worker/src/signal_scorer.rs` içinde yeni `source_key` dalı veya jenerik ayrıştırıcı ekleyin.

## Yeni HTTP kaynak eklemek

1. `external_data_sources` satırı ekleyin (`POST /api/v1/analysis/external-fetch/sources` — ops rolü, veya SQL migration).
2. Worker `QTSS_EXTERNAL_FETCH=1` ve `external_fetch_engine` tick ile çekim yapılır; sonuç `data_snapshots.source_key` = satırın `key` alanı.
3. Confluence veya başka skorlayıcıda bu anahtarı okumak için `crates/qtss-worker/src/confluence.rs` (veya ileride `SignalScorer`) genişletilir.

## İlgili plan

[`PLAN_CONFLUENCE_AND_MARKET_DATA.md`](./PLAN_CONFLUENCE_AND_MARKET_DATA.md) — §1 kaynak matrisi, §3 `DataSourceProvider`, §4 confluence.
