//! Setup card PNG renderer for Telegram delivery (and Faz 9.0 X feed).
//!
//! Pure synchronous function: takes the setup snapshot + recent bars
//! + event type and returns PNG bytes. Profile colors and locale
//! labels live in small dispatch tables (CLAUDE.md #1) — no scattered
//! if/else, no hardcoded copy embedded in branches.
//!
//! The visual is intentionally minimal for Faz 8.0; the Faz 9.0 X
//! feed reuses this module, so the public API (`render_setup_card`)
//! is stable while internals can evolve.

use plotters::prelude::*;
use qtss_storage::{MarketBarRow, V2SetupRow};
use rust_decimal::prelude::ToPrimitive;

pub struct SetupCardInput<'a> {
    pub setup: &'a V2SetupRow,
    /// Chronological, oldest first.
    pub bars: &'a [MarketBarRow],
    /// "opened" | "updated" | "closed"
    pub event_type: &'a str,
    pub current_price: Option<f64>,
    /// "tr" | "en"
    pub locale: &'a str,
}

// ---------- dispatch tables ----------

#[derive(Clone, Copy)]
#[allow(dead_code)] // `risk` reserved for Faz 8.0.x footer expansion.
struct LabelPack {
    opened: &'static str,
    updated: &'static str,
    closed: &'static str,
    long: &'static str,
    short: &'static str,
    entry: &'static str,
    stop: &'static str,
    target: &'static str,
    koruma: &'static str,
    risk: &'static str,
}

const TR_PACK: LabelPack = LabelPack {
    opened: "AÇILDI",
    updated: "GÜNCELLENDİ",
    closed: "KAPANDI",
    long: "LONG",
    short: "SHORT",
    entry: "Giriş",
    stop: "Stop",
    target: "Hedef",
    koruma: "Koruma",
    risk: "Risk",
};

const EN_PACK: LabelPack = LabelPack {
    opened: "OPENED",
    updated: "UPDATED",
    closed: "CLOSED",
    long: "LONG",
    short: "SHORT",
    entry: "Entry",
    stop: "Stop",
    target: "Target",
    koruma: "Trail",
    risk: "Risk",
};

fn pack_for(locale: &str) -> LabelPack {
    match locale {
        "en" => EN_PACK,
        _ => TR_PACK,
    }
}

fn event_label(pack: &LabelPack, event_type: &str) -> &'static str {
    match event_type {
        "opened" => pack.opened,
        "updated" => pack.updated,
        "closed" => pack.closed,
        _ => "",
    }
}

/// Profile → accent color dispatch table.
fn profile_color(profile: &str) -> RGBColor {
    match profile {
        "T" | "t" => RGBColor(0xF9, 0x73, 0x16), // orange
        "Q" | "q" => RGBColor(0x3B, 0x82, 0xF6), // blue
        "D" | "d" => RGBColor(0xA8, 0x55, 0xF7), // purple
        _ => RGBColor(0x9C, 0xA3, 0xAF),
    }
}

fn profile_label(profile: &str) -> &'static str {
    match profile {
        "T" | "t" => "T",
        "Q" | "q" => "Q",
        "D" | "d" => "D",
        _ => "?",
    }
}

fn direction_label(pack: &LabelPack, direction: &str) -> &'static str {
    match direction {
        "long" => pack.long,
        "short" => pack.short,
        _ => "",
    }
}

// ---------- public entrypoint ----------

/// Infallible: always returns PNG bytes. On any rendering error it
/// falls back to a solid-color placeholder so the telegram dispatcher
/// never blocks on rendering.
pub fn render_setup_card(input: &SetupCardInput<'_>) -> Vec<u8> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| render_inner(input))) {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(e)) => {
            tracing::warn!(%e, "setup_chart: render_inner failed, using fallback");
            render_fallback(input)
        }
        Err(_) => {
            tracing::warn!("setup_chart: render panicked, using fallback");
            render_fallback(input)
        }
    }
}

// ---------- constants (visual only — not business config) ----------

const W: u32 = 1080;
const H: u32 = 720;
const HEADER_H: i32 = 80;
const FOOTER_H: i32 = 80;
const BG: RGBColor = RGBColor(0x0E, 0x11, 0x16);
const FG: RGBColor = RGBColor(0xC9, 0xD1, 0xD9);
const UP: RGBColor = RGBColor(0x4A, 0xDE, 0x80);
const DN: RGBColor = RGBColor(0xF8, 0x7F, 0x71);
const WHITE: RGBColor = RGBColor(0xFF, 0xFF, 0xFF);
const RED: RGBColor = RGBColor(0xEF, 0x44, 0x44);
const GREEN: RGBColor = RGBColor(0x22, 0xC5, 0x5E);
const ORANGE: RGBColor = RGBColor(0xF9, 0x73, 0x16);

fn style(color: &RGBColor, size: f64) -> plotters::style::TextStyle<'_> {
    TextStyle::from(("sans-serif", size).into_font()).color(color)
}

// ---------- full renderer ----------

fn render_inner(input: &SetupCardInput<'_>) -> Result<Vec<u8>, String> {
    let tmp = tempfile::Builder::new()
        .suffix(".png")
        .tempfile()
        .map_err(|e| e.to_string())?;
    let path = tmp.path().to_path_buf();
    {
        let root = BitMapBackend::new(&path, (W, H)).into_drawing_area();
        root.fill(&BG).map_err(|e| format!("{e:?}"))?;

        let pack = pack_for(input.locale);
        let accent = profile_color(&input.setup.profile);

        // ---- header ----
        root.draw(&Rectangle::new(
            [(0, 0), (W as i32, HEADER_H)],
            accent.filled(),
        ))
        .map_err(|e| format!("{e:?}"))?;
        let header_left = format!(
            "{} · {} · {}",
            input.setup.symbol,
            input.setup.timeframe,
            profile_label(&input.setup.profile)
        );
        root.draw(&Text::new(header_left, (20, 22), style(&WHITE, 28.0)))
            .map_err(|e| format!("{e:?}"))?;
        let header_right = format!(
            "{}  {}",
            input.setup.exchange.to_uppercase(),
            event_label(&pack, input.event_type)
        );
        root.draw(&Text::new(
            header_right,
            (W as i32 - 360, 28),
            style(&WHITE, 22.0),
        ))
        .map_err(|e| format!("{e:?}"))?;

        // ---- body (candles) ----
        let body_top = HEADER_H + 10;
        let body_bottom = H as i32 - FOOTER_H - 10;
        let body_left = 60i32;
        let body_right = W as i32 - 220; // leave right margin for level labels

        let bars = input.bars;
        if !bars.is_empty() {
            let highs: Vec<f64> = bars.iter().map(|b| b.high.to_f64().unwrap_or(0.0)).collect();
            let lows: Vec<f64> = bars.iter().map(|b| b.low.to_f64().unwrap_or(0.0)).collect();
            let mut ymin = lows.iter().copied().fold(f64::INFINITY, f64::min);
            let mut ymax = highs.iter().copied().fold(f64::NEG_INFINITY, f64::max);

            // Include setup levels in Y range so they're always visible.
            let levels = [
                input.setup.entry_price.map(|v| v as f64),
                input.setup.entry_sl.map(|v| v as f64),
                input.setup.koruma.map(|v| v as f64),
                input.setup.target_ref.map(|v| v as f64),
                input.current_price,
            ];
            for opt in levels.iter().flatten().copied() {
                if opt.is_finite() && opt > 0.0 {
                    ymin = ymin.min(opt);
                    ymax = ymax.max(opt);
                }
            }
            if !(ymax > ymin) {
                ymax = ymin + 1.0;
            }
            let pad = (ymax - ymin) * 0.05;
            ymin -= pad;
            ymax += pad;

            let n = bars.len().max(1) as f64;
            let body_w = (body_right - body_left) as f64;
            let body_h = (body_bottom - body_top) as f64;
            let candle_w = (body_w / n).max(1.0);
            let to_y = |price: f64| -> i32 {
                let t = (price - ymin) / (ymax - ymin);
                body_bottom - (t * body_h) as i32
            };

            for (i, b) in bars.iter().enumerate() {
                let x_center = body_left + ((i as f64 + 0.5) * candle_w) as i32;
                let o = b.open.to_f64().unwrap_or(0.0);
                let h_ = b.high.to_f64().unwrap_or(0.0);
                let l = b.low.to_f64().unwrap_or(0.0);
                let c = b.close.to_f64().unwrap_or(0.0);
                let color = if c >= o { UP } else { DN };
                // wick
                root.draw(&PathElement::new(
                    vec![(x_center, to_y(h_)), (x_center, to_y(l))],
                    color.stroke_width(1),
                ))
                .map_err(|e| format!("{e:?}"))?;
                // body
                let half = ((candle_w * 0.35) as i32).max(1);
                let (y1, y2) = if c >= o { (to_y(c), to_y(o)) } else { (to_y(o), to_y(c)) };
                root.draw(&Rectangle::new(
                    [(x_center - half, y1), (x_center + half, y2.max(y1 + 1))],
                    color.filled(),
                ))
                .map_err(|e| format!("{e:?}"))?;
            }

            // level lines dispatch table
            let level_defs: [(Option<f64>, RGBColor, &str, bool); 4] = [
                (input.setup.entry_price.map(|v| v as f64), WHITE, pack.entry, false),
                (input.setup.entry_sl.map(|v| v as f64), RED, pack.stop, true),
                (input.setup.target_ref.map(|v| v as f64), GREEN, pack.target, false),
                (input.setup.koruma.map(|v| v as f64), ORANGE, pack.koruma, false),
            ];
            for (opt, color, label, _dashed) in level_defs.iter() {
                let Some(v) = opt else { continue };
                if !v.is_finite() || *v <= 0.0 {
                    continue;
                }
                let y = to_y(*v);
                root.draw(&PathElement::new(
                    vec![(body_left, y), (body_right, y)],
                    color.stroke_width(1),
                ))
                .map_err(|e| format!("{e:?}"))?;
                let txt = format!("{} {:.4}", label, v);
                root.draw(&Text::new(txt, (body_right + 8, y - 8), style(color, 16.0)))
                    .map_err(|e| format!("{e:?}"))?;
            }
        } else {
            root.draw(&Text::new(
                "no bars",
                (body_left + 20, body_top + 40),
                style(&FG, 20.0),
            ))
            .map_err(|e| format!("{e:?}"))?;
        }

        // ---- footer ----
        let pack = pack_for(input.locale);
        let footer_y = H as i32 - FOOTER_H;
        root.draw(&Rectangle::new(
            [(0, footer_y), (W as i32, footer_y + 4)],
            accent.filled(),
        ))
        .map_err(|e| format!("{e:?}"))?;
        let alt_txt = input
            .setup
            .alt_type
            .as_deref()
            .map(|a| format!(" · {a}"))
            .unwrap_or_default();
        let r_mult = compute_r_multiple(input);
        let r_txt = r_mult
            .map(|r| format!(" · R {:+.2}", r))
            .unwrap_or_default();
        let footer_text = format!(
            "{}{} · {} · {}{}",
            direction_label(&pack, &input.setup.direction),
            alt_txt,
            input.setup.state,
            event_label(&pack, input.event_type),
            r_txt
        );
        root.draw(&Text::new(
            footer_text,
            (20, footer_y + 24),
            style(&FG, 26.0),
        ))
        .map_err(|e| format!("{e:?}"))?;
        if let Some(reason) = &input.setup.close_reason {
            if input.event_type == "closed" {
                root.draw(&Text::new(
                    format!("({reason})"),
                    (20, footer_y + 56),
                    style(&FG, 18.0),
                ))
                .map_err(|e| format!("{e:?}"))?;
            }
        }

        root.present().map_err(|e| format!("{e:?}"))?;
    }
    std::fs::read(&path).map_err(|e| e.to_string())
}

fn compute_r_multiple(input: &SetupCardInput<'_>) -> Option<f64> {
    let entry = input.setup.entry_price? as f64;
    let sl = input.setup.entry_sl? as f64;
    let price = input.current_price.or_else(|| {
        input
            .bars
            .last()
            .map(|b| b.close.to_f64().unwrap_or(0.0))
    })?;
    let risk = (entry - sl).abs();
    if !(risk > 0.0) {
        return None;
    }
    let dir_sign = match input.setup.direction.as_str() {
        "long" => 1.0,
        "short" => -1.0,
        _ => return None,
    };
    Some(((price - entry) * dir_sign) / risk)
}

// ---------- fallback (guaranteed valid PNG) ----------

fn render_fallback(input: &SetupCardInput<'_>) -> Vec<u8> {
    // TODO Faz 8.0.x: richer fallback (e.g. embed text). This path is
    // only taken on panics/IO failures so keeping it dead simple.
    let tmp = tempfile::Builder::new()
        .suffix(".png")
        .tempfile()
        .ok();
    if let Some(tmp) = tmp {
        let path = tmp.path().to_path_buf();
        let done = {
            let root = BitMapBackend::new(&path, (W, H)).into_drawing_area();
            let ok = root.fill(&BG).is_ok();
            let title = format!("Setup Card · {}", input.setup.symbol);
            let _ = root.draw(&Text::new(title, (40, 40), style(&FG, 28.0)));
            ok && root.present().is_ok()
        };
        if done {
            if let Ok(bytes) = std::fs::read(&path) {
                return bytes;
            }
        }
    }
    // Absolute last-resort: a single-pixel PNG (1x1, grey).
    // Bytes below form a valid 1x1 grey PNG.
    vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x00, 0x00, 0x00, 0x00, 0x3A,
        0x7E, 0x9B, 0x55, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ]
}
