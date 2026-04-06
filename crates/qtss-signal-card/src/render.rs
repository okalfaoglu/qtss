//! Bitmap card layout (reference: Telegram signal share mock).

use plotters::prelude::*;
use std::path::Path;

const DEFAULT_W: u32 = 720;
const DEFAULT_H: u32 = 520;
const HEADER_H: i32 = 54;
const ROW0: i32 = HEADER_H + 6;
const ROW_H: i32 = 76;
const LABEL_X: i32 = 14;
const VALUE_X: i32 = 232;

fn pe<E: std::fmt::Debug>(e: E) -> SignalCardRenderError {
    SignalCardRenderError::Plotters(format!("{e:?}"))
}

#[derive(Debug, Clone)]
pub struct SignalCardParams {
    pub header_left: String,
    pub header_right: String,
    pub price_caption: String,
    pub price_line_positive: bool,
    pub side_label: String,
    pub strength_10: u8,
    pub strength_word: String,
    pub subscores_line: String,
    pub entry_line: String,
    pub stop_line: String,
    pub tp_line: String,
    /// When 0, [`DEFAULT_W`]×[`DEFAULT_H`] is used (720×520).
    pub canvas_width: u32,
    /// When 0, [`DEFAULT_H`] is used.
    pub canvas_height: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum SignalCardRenderError {
    #[error("plotters: {0}")]
    Plotters(String),
    #[error("io: {0}")]
    Io(String),
}

/// Title / row / small body text (DejaVu Sans when fontconfig finds it).
fn style_title(color: &RGBColor) -> plotters::style::TextStyle<'_> {
    TextStyle::from(("DejaVu Sans", 21.0).into_font()).color(color)
}

fn style_row(color: &RGBColor) -> plotters::style::TextStyle<'_> {
    TextStyle::from(("DejaVu Sans", 17.0).into_font()).color(color)
}

fn style_small(color: &RGBColor) -> plotters::style::TextStyle<'_> {
    TextStyle::from(("DejaVu Sans", 14.0).into_font()).color(color)
}

/// Renders a PNG (RGB) suitable for Telegram `sendPhoto`.
///
/// Wraps the plotters call in `catch_unwind` so a missing font degrades to an
/// `Err` instead of killing a tokio worker thread.
pub fn render_signal_card_png(params: &SignalCardParams) -> Result<Vec<u8>, SignalCardRenderError> {
    let params = params.clone();
    match std::panic::catch_unwind(move || render_signal_card_inner(&params)) {
        Ok(result) => result,
        Err(payload) => {
            let msg = payload
                .downcast_ref::<String>()
                .map(|s| s.as_str())
                .or_else(|| payload.downcast_ref::<&str>().copied())
                .unwrap_or("unknown panic");
            Err(SignalCardRenderError::Plotters(format!(
                "render panicked: {msg}"
            )))
        }
    }
}

fn render_signal_card_inner(params: &SignalCardParams) -> Result<Vec<u8>, SignalCardRenderError> {
    let w = if params.canvas_width > 0 {
        params.canvas_width
    } else {
        DEFAULT_W
    };
    let h = if params.canvas_height > 0 {
        params.canvas_height
    } else {
        DEFAULT_H
    };
    let tmp = tempfile::Builder::new()
        .suffix(".png")
        .tempfile()
        .map_err(|e| SignalCardRenderError::Io(e.to_string()))?;
    let path: &Path = tmp.path();

    {
        let root = BitMapBackend::new(path, (w, h)).into_drawing_area();
        root.fill(&RGBColor(5, 5, 5)).map_err(pe)?;

        let blue = RGBColor(59, 113, 243);
        root.draw(&Rectangle::new([(0, 0), (w as i32, HEADER_H)], blue.filled()))
            .map_err(pe)?;

        let white = RGBColor(248, 250, 252);
        root.draw(&Text::new(
            params.header_left.as_str(),
            (16, 16),
            style_title(&white),
        ))
        .map_err(pe)?;

        let hw = estimate_text_width(params.header_right.as_str(), 21.0);
        let hx = (w as i32 - hw - 16).max(16);
        root.draw(&Text::new(
            params.header_right.as_str(),
            (hx, 16),
            style_title(&white),
        ))
        .map_err(pe)?;

        let label_c = RGBColor(176, 180, 186);
        let y1 = ROW0;
        let sep_y1 = y1 - 6 + ROW_H;
        root.draw(&Rectangle::new(
            [(0, sep_y1), (w as i32, sep_y1 + 1)],
            RGBColor(48, 48, 48).filled(),
        ))
        .map_err(pe)?;

        root.draw(&Text::new(
            "Güncel Fiyat:",
            (LABEL_X, y1 + 8),
            style_row(&label_c),
        ))
        .map_err(pe)?;

        let price_color = if params.price_line_positive {
            RGBColor(34, 197, 94)
        } else {
            RGBColor(248, 113, 113)
        };
        root.draw(&Text::new(
            params.price_caption.as_str(),
            (VALUE_X, y1 + 8),
            style_row(&price_color),
        ))
        .map_err(pe)?;

        let y2 = y1 + ROW_H;
        let sep_y2 = y2 - 6 + ROW_H + 18;
        root.draw(&Rectangle::new(
            [(0, sep_y2), (w as i32, sep_y2 + 1)],
            RGBColor(48, 48, 48).filled(),
        ))
        .map_err(pe)?;

        root.draw(&Text::new(
            "Durum:",
            (LABEL_X, y2 + 8),
            style_row(&label_c),
        ))
        .map_err(pe)?;

        let gold = RGBColor(250, 204, 21);
        root.draw(&Text::new(
            params.side_label.as_str(),
            (VALUE_X, y2 + 6),
            style_row(&gold),
        ))
        .map_err(pe)?;

        let filled = params.strength_10.min(10);
        let box_w = 13_i32;
        let gap = 4_i32;
        let yellow = RGBColor(255, 214, 0);
        let dim = RGBColor(55, 55, 55);
        let bx0 = VALUE_X + 56;
        let by0 = y2 + 6;
        for i in 0..10 {
            let x = bx0 + i * (box_w + gap);
            let y = by0;
            let fill = if (i as u8) < filled {
                yellow.filled()
            } else {
                dim.filled()
            };
            root.draw(&Rectangle::new([(x, y), (x + box_w, y + box_w)], fill))
                .map_err(pe)?;
            root.draw(&Rectangle::new(
                [(x, y), (x + box_w, y + box_w)],
                RGBColor(90, 90, 90).stroke_width(1),
            ))
            .map_err(pe)?;
        }

        let after_bar = format!(" {}/10 {}", params.strength_10, params.strength_word);
        let bx = VALUE_X + 56 + 10 * (13 + 4) + 8;
        root.draw(&Text::new(
            after_bar.as_str(),
            (bx, y2 + 8),
            style_row(&gold),
        ))
        .map_err(pe)?;

        root.draw(&Text::new(
            params.subscores_line.as_str(),
            (VALUE_X, y2 + 32),
            style_small(&gold),
        ))
        .map_err(pe)?;

        let y3 = y2 + ROW_H + 18;
        let sep_y3 = y3 - 6 + ROW_H;
        root.draw(&Rectangle::new(
            [(0, sep_y3), (w as i32, sep_y3 + 1)],
            RGBColor(48, 48, 48).filled(),
        ))
        .map_err(pe)?;

        root.draw(&Text::new(
            "Giriş (Ort):",
            (LABEL_X, y3 + 8),
            style_row(&label_c),
        ))
        .map_err(pe)?;
        root.draw(&Text::new(
            params.entry_line.as_str(),
            (VALUE_X, y3 + 8),
            style_row(&gold),
        ))
        .map_err(pe)?;

        let y4 = y3 + ROW_H;
        let sep_y4 = y4 - 6 + ROW_H;
        root.draw(&Rectangle::new(
            [(0, sep_y4), (w as i32, sep_y4 + 1)],
            RGBColor(48, 48, 48).filled(),
        ))
        .map_err(pe)?;

        root.draw(&Text::new(
            "Stop (SL):",
            (LABEL_X, y4 + 8),
            style_row(&label_c),
        ))
        .map_err(pe)?;
        let red = RGBColor(251, 113, 133);
        root.draw(&Text::new(
            params.stop_line.as_str(),
            (VALUE_X, y4 + 8),
            style_row(&red),
        ))
        .map_err(pe)?;

        let y5 = y4 + ROW_H;
        let sep_y5 = y5 - 6 + ROW_H;
        root.draw(&Rectangle::new(
            [(0, sep_y5), (w as i32, sep_y5 + 1)],
            RGBColor(48, 48, 48).filled(),
        ))
        .map_err(pe)?;

        root.draw(&Text::new(
            "Kar Al (TP):",
            (LABEL_X, y5 + 8),
            style_row(&label_c),
        ))
        .map_err(pe)?;
        let green = RGBColor(52, 211, 153);
        root.draw(&Text::new(
            params.tp_line.as_str(),
            (VALUE_X, y5 + 8),
            style_row(&green),
        ))
        .map_err(pe)?;

        root.present().map_err(pe)?;
    }

    std::fs::read(path).map_err(|e| SignalCardRenderError::Io(e.to_string()))
}

/// Rough width estimate for right-aligning header (no font metrics API in plotters).
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
