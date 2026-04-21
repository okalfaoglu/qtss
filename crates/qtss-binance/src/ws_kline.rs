//! Tek akış / birleşik akış kline WebSocket metin çerçeveleri.

use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct ClosedKline {
    pub symbol: String,
    pub interval: String,
    pub open_time_ms: i64,
    pub open: String,
    pub high: String,
    pub low: String,
    pub close: String,
    pub volume: String,
    pub quote_volume: Option<String>,
    pub trade_count: Option<u64>,
}

/// Any kline frame — closed OR still forming. `is_final=true` means the bar
/// just closed and should be archived into `market_bars`. `is_final=false`
/// is a running tick and should overwrite `market_bars_open` only.
#[derive(Debug, Clone)]
pub struct KlineFrame {
    pub symbol: String,
    pub interval: String,
    pub open_time_ms: i64,
    pub close_time_ms: i64,
    pub open: String,
    pub high: String,
    pub low: String,
    pub close: String,
    pub volume: String,
    pub quote_volume: Option<String>,
    pub trade_count: Option<u64>,
    pub is_final: bool,
}

#[derive(Debug, Deserialize)]
struct KlineEventTop {
    #[serde(rename = "e")]
    event_type: Option<String>,
    s: Option<String>,
    k: Option<KlineK>,
}

#[derive(Debug, Deserialize)]
struct KlineK {
    t: i64,
    #[serde(rename = "T")]
    close_time: Option<i64>,
    s: Option<String>,
    #[serde(rename = "i")]
    interval: Option<String>,
    o: String,
    h: String,
    l: String,
    c: String,
    v: String,
    q: Option<String>,
    n: Option<u64>,
    #[serde(rename = "x")]
    is_final: Option<bool>,
}

/// `data` sarmalayıcı (birleşik akış) veya düz `kline` olayı.
pub fn parse_closed_kline_json(text: &str) -> Option<ClosedKline> {
    let frame = parse_kline_frame_json(text)?;
    if !frame.is_final {
        return None;
    }
    Some(ClosedKline {
        symbol: frame.symbol,
        interval: frame.interval,
        open_time_ms: frame.open_time_ms,
        open: frame.open,
        high: frame.high,
        low: frame.low,
        close: frame.close,
        volume: frame.volume,
        quote_volume: frame.quote_volume,
        trade_count: frame.trade_count,
    })
}

/// Same shape as `parse_closed_kline_json` but returns every frame — open or
/// closed — with an `is_final` flag. Lets the worker route final frames to
/// the archive (`market_bars`) and running frames to the live ticker
/// (`market_bars_open`) off the same WebSocket stream, without doubling
/// the subscription count.
pub fn parse_kline_frame_json(text: &str) -> Option<KlineFrame> {
    let top: serde_json::Value = serde_json::from_str(text).ok()?;
    let ev = if let Some(d) = top.get("data") {
        serde_json::from_value::<KlineEventTop>(d.clone()).ok()?
    } else {
        serde_json::from_value::<KlineEventTop>(top).ok()?
    };
    if ev.event_type.as_deref() != Some("kline") {
        return None;
    }
    let k = ev.k?;
    let symbol = k.s.or(ev.s).filter(|s| !s.is_empty())?;
    let interval = k.interval?.to_string();
    Some(KlineFrame {
        symbol,
        interval,
        open_time_ms: k.t,
        // Binance may omit `T` for single-stream frames; fall back to
        // open_time so the ticker row still carries a sane close_time.
        close_time_ms: k.close_time.unwrap_or(k.t),
        open: k.o,
        high: k.h,
        low: k.l,
        close: k.c,
        volume: k.v,
        quote_volume: k.q,
        trade_count: k.n,
        is_final: k.is_final.unwrap_or(false),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn closed_k_payload(x: bool, symbol_top: Option<&str>) -> serde_json::Value {
        let mut top = json!({
            "e": "kline",
            "E": 1_640_000_000u64,
            "k": {
                "t": 1_640_000_000_000i64,
                "T": 1_640_000_059_999i64,
                "i": "1m",
                "o": "42000.10",
                "h": "42100.00",
                "l": "41900.00",
                "c": "42050.00",
                "v": "123.45",
                "q": "5180000.1",
                "n": 999u64,
                "x": x
            }
        });
        if let Some(s) = symbol_top {
            top["s"] = json!(s);
        } else {
            top["k"]["s"] = json!("BTCUSDT");
        }
        top
    }

    #[test]
    fn parses_closed_single_stream() {
        let text = closed_k_payload(true, Some("BTCUSDT")).to_string();
        let k = parse_closed_kline_json(&text).expect("closed");
        assert_eq!(k.symbol, "BTCUSDT");
        assert_eq!(k.interval, "1m");
        assert_eq!(k.open_time_ms, 1_640_000_000_000);
        assert_eq!(k.open, "42000.10");
        assert_eq!(k.quote_volume.as_deref(), Some("5180000.1"));
        assert_eq!(k.trade_count, Some(999));
    }

    #[test]
    fn ignores_open_kline() {
        let text = closed_k_payload(false, Some("ETHUSDT")).to_string();
        assert!(parse_closed_kline_json(&text).is_none());
    }

    #[test]
    fn parses_combined_wrapper() {
        let inner = closed_k_payload(true, None);
        let wrapped = json!({ "stream": "btcusdt@kline_1m", "data": inner });
        let k = parse_closed_kline_json(&wrapped.to_string()).expect("combined");
        assert_eq!(k.symbol, "BTCUSDT");
        assert_eq!(k.close, "42050.00");
    }

    #[test]
    fn rejects_non_kline_event() {
        let v = json!({ "e": "trade", "s": "BTCUSDT" });
        assert!(parse_closed_kline_json(&v.to_string()).is_none());
    }
}
