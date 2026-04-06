//! PNG card for operational AI decisions (action + risk knobs, same visual language as tactical approval).

use plotters::prelude::*;
use std::path::Path;

use crate::render::SignalCardRenderError;
use crate::subscores::strength_label_tr;
use crate::format_compact_price;

const OP_W: u32 = 880;
const OP_H: u32 = 600;

fn pe<E: std::fmt::Debug>(e: E) -> SignalCardRenderError {
    SignalCardRenderError::Plotters(format!("{e:?}"))
}

fn style_title(color: &RGBColor) -> plotters::style::TextStyle<'_> {
    TextStyle::from(("DejaVu Sans", 21.0).into_font()).color(color)
}

fn style_row(color: &RGBColor) -> plotters::style::TextStyle<'_> {
    TextStyle::from(("DejaVu Sans", 17.0).into_font()).color(color)
}

fn style_small(color: &RGBColor) -> plotters::style::TextStyle<'_> {
    TextStyle::from(("DejaVu Sans", 14.0).into_font()).color(color)
}

/// Operational layer snapshot for Telegram PNG.
#[derive(Debug, Clone)]
pub struct OperationalApprovalCardInput {
    pub symbol: String,
    pub timeframe: String,
    pub last_close: Option<f64>,
    pub approx_change_pct: Option<f64>,
    /// Snake-case action from JSON (e.g. `tighten_stop`).
    pub action: String,
    pub confidence_0_1: f64,
    pub new_sl_pct: Option<f64>,
    pub new_tp_pct: Option<f64>,
    pub trailing_callback_pct: Option<f64>,
    pub partial_close_pct: Option<f64>,
}

fn action_display(snake: &str) -> String {
    snake
        .trim()
        .to_ascii_uppercase()
        .replace('_', " ")
}

fn estimate_text_width(s: &str, px: f64) -> i32 {
    let mut w = 0.0_f64;
    for ch in s.chars() {
        w += if ch.is_ascii() {
            if ch.is_ascii_uppercase() {
                0.62
            } else {
                0.55
            }
        } else {
            0.9
        } * px;
    }
    w as i32
}

/// Renders operational approval card (880×600).
pub fn try_render_operational_approval_card_png(
    input: &OperationalApprovalCardInput,
) -> Result<Vec<u8>, SignalCardRenderError> {
    let tmp = tempfile::Builder::new()
        .suffix(".png")
        .tempfile()
        .map_err(|e| SignalCardRenderError::Io(e.to_string()))?;
    let path: &Path = tmp.path();
    let w = OP_W as i32;
    let h = OP_H as i32;
    const HEADER_H: i32 = 54;
    const LABEL_X: i32 = 14;
    const VALUE_X: i32 = 232;
    const ROW_H: i32 = 52;
    let y0 = HEADER_H + 8;

    {
        let root = BitMapBackend::new(path, (OP_W, OP_H)).into_drawing_area();
        root.fill(&RGBColor(5, 5, 5)).map_err(pe)?;

        let blue = RGBColor(59, 113, 243);
        root.draw(&Rectangle::new([(0, 0), (w, HEADER_H)], blue.filled()))
            .map_err(pe)?;

        let white = RGBColor(248, 250, 252);
        let head_left = format!(
            "{} ({})",
            input.symbol.trim().to_uppercase(),
            input.timeframe
        );
        root.draw(&Text::new(head_left.as_str(), (16, 16), style_title(&white)))
            .map_err(pe)?;
        let hr = "OPERASYONEL AI";
        let hw = estimate_text_width(hr, 21.0);
        let hx = (w - hw - 16).max(16);
        root.draw(&Text::new(hr, (hx, 16), style_title(&white)))
            .map_err(pe)?;

        let label_c = RGBColor(176, 180, 186);
        let mut y = y0;

        root.draw(&Rectangle::new(
            [(0, y - 6), (w, y - 5)],
            RGBColor(48, 48, 48).filled(),
        ))
        .map_err(pe)?;
        let price_line = match input.last_close {
            Some(p) if p.is_finite() => {
                let chg = input.approx_change_pct.unwrap_or(0.0);
                let tri = if chg >= 0.0 { "▲" } else { "▼" };
                format!(
                    "{}  {} {:+.2}%",
                    format_compact_price(p),
                    tri,
                    chg
                )
            }
            _ => "—".to_string(),
        };
        let price_pos = input.approx_change_pct.unwrap_or(0.0) >= 0.0;
        let price_color = if price_pos {
            RGBColor(52, 211, 153)
        } else {
            RGBColor(248, 113, 113)
        };
        root.draw(&Text::new("Güncel Fiyat:", (LABEL_X, y + 6), style_row(&label_c)))
            .map_err(pe)?;
        root.draw(&Text::new(price_line.as_str(), (VALUE_X, y + 6), style_row(&price_color)))
            .map_err(pe)?;

        y += ROW_H;
        root.draw(&Rectangle::new(
            [(0, y - 4), (w, y - 3)],
            RGBColor(48, 48, 48).filled(),
        ))
        .map_err(pe)?;
        let gold = RGBColor(250, 204, 21);
        let act = action_display(&input.action);
        root.draw(&Text::new("Eylem:", (LABEL_X, y + 6), style_row(&label_c)))
            .map_err(pe)?;
        root.draw(&Text::new(act.as_str(), (VALUE_X, y + 4), style_row(&gold)))
            .map_err(pe)?;

        y += ROW_H;
        root.draw(&Rectangle::new(
            [(0, y - 4), (w, y - 3)],
            RGBColor(48, 48, 48).filled(),
        ))
        .map_err(pe)?;
        let strength = (input.confidence_0_1 * 10.0).round().clamp(1.0, 10.0) as u8;
        let strength_word = strength_label_tr(strength).to_string();
        root.draw(&Text::new("Güven:", (LABEL_X, y + 6), style_row(&label_c)))
            .map_err(pe)?;
        root.draw(
            &Text::new(
                format!("{strength}/10 {strength_word}").as_str(),
                (VALUE_X, y + 6),
                style_row(&gold),
            ),
        )
        .map_err(pe)?;

        let box_w = 13_i32;
        let gap = 4_i32;
        let yellow = RGBColor(255, 214, 0);
        let dim = RGBColor(55, 55, 55);
        let bx0 = VALUE_X + 200;
        let by0 = y + 4;
        let filled = strength.min(10);
        for i in 0..10 {
            let x = bx0 + i * (box_w + gap);
            let fill = if (i as u8) < filled {
                yellow.filled()
            } else {
                dim.filled()
            };
            root.draw(&Rectangle::new([(x, by0), (x + box_w, by0 + box_w)], fill))
                .map_err(pe)?;
            root.draw(&Rectangle::new(
                [(x, by0), (x + box_w, by0 + box_w)],
                RGBColor(90, 90, 90).stroke_width(1),
            ))
            .map_err(pe)?;
        }

        fn fmt_pct_opt(x: Option<f64>) -> String {
            x.map(|v| format!("{v:.4}%"))
                .unwrap_or_else(|| "—".to_string())
        }

        y += ROW_H + 8;
        root.draw(&Rectangle::new(
            [(0, y - 4), (w, y - 3)],
            RGBColor(48, 48, 48).filled(),
        ))
        .map_err(pe)?;
        let red = RGBColor(251, 113, 133);
        root.draw(&Text::new("Yeni SL %:", (LABEL_X, y + 6), style_row(&label_c)))
            .map_err(pe)?;
        root.draw(
            &Text::new(
                fmt_pct_opt(input.new_sl_pct).as_str(),
                (VALUE_X, y + 6),
                style_row(&red),
            ),
        )
        .map_err(pe)?;

        y += ROW_H;
        root.draw(&Rectangle::new(
            [(0, y - 4), (w, y - 3)],
            RGBColor(48, 48, 48).filled(),
        ))
        .map_err(pe)?;
        let green = RGBColor(52, 211, 153);
        root.draw(&Text::new("Yeni TP %:", (LABEL_X, y + 6), style_row(&label_c)))
            .map_err(pe)?;
        root.draw(
            &Text::new(
                fmt_pct_opt(input.new_tp_pct).as_str(),
                (VALUE_X, y + 6),
                style_row(&green),
            ),
        )
        .map_err(pe)?;

        y += ROW_H;
        root.draw(&Rectangle::new(
            [(0, y - 4), (w, y - 3)],
            RGBColor(48, 48, 48).filled(),
        ))
        .map_err(pe)?;
        root.draw(&Text::new("Trailing %:", (LABEL_X, y + 6), style_row(&label_c)))
            .map_err(pe)?;
        root.draw(
            &Text::new(
                fmt_pct_opt(input.trailing_callback_pct).as_str(),
                (VALUE_X, y + 6),
                style_row(&white),
            ),
        )
        .map_err(pe)?;

        y += ROW_H;
        root.draw(&Rectangle::new(
            [(0, y - 4), (w, y - 3)],
            RGBColor(48, 48, 48).filled(),
        ))
        .map_err(pe)?;
        root.draw(&Text::new("Kısmi kapat %:", (LABEL_X, y + 6), style_row(&label_c)))
            .map_err(pe)?;
        root.draw(
            &Text::new(
                fmt_pct_opt(input.partial_close_pct).as_str(),
                (VALUE_X, y + 6),
                style_row(&white),
            ),
        )
        .map_err(pe)?;

        y += ROW_H;
        root.draw(&Rectangle::new(
            [(0, y - 4), (w, y - 3)],
            RGBColor(48, 48, 48).filled(),
        ))
        .map_err(pe)?;
        root.draw(&Text::new(
            "AI ONAY — operasyonel düzenleme",
            (LABEL_X, y + 4),
            style_small(&label_c),
        ))
        .map_err(pe)?;

        root.present().map_err(pe)?;
    }

    std::fs::read(path).map_err(|e| SignalCardRenderError::Io(e.to_string()))
}
