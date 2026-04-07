//! PNG card for AI tactical decisions pending approval (wider canvas than default signal share).

use crate::render::{render_signal_card_png, SignalCardParams, SignalCardRenderError};
use crate::subscores::strength_label_tr;

const AI_CARD_W: u32 = 880;
const AI_CARD_H: u32 = 540;

/// Numeric snapshot for a tactical-style AI trade plan (levels vs reference price).
#[derive(Debug, Clone)]
pub struct AiApprovalCardInput {
    pub symbol: String,
    pub timeframe: String,
    pub last_close: f64,
    pub approx_change_pct: Option<f64>,
    /// When true, LONG-style coloring and SL/TP geometry; when false, SHORT.
    /// Ignored when [`Self::flat_no_trade`] is true.
    pub side_long: bool,
    pub confidence_0_1: f64,
    /// Entry / reference used for Giriş row and R:R percentages (often `last_close` or hint).
    pub reference_price: f64,
    pub stop_loss: f64,
    pub take_profit: f64,
    /// No directional trade (`neutral` / `no_trade`): card shows FLAT and em-dash SL/TP rows.
    pub flat_no_trade: bool,
}

#[must_use]
pub fn format_compact_price(x: f64) -> String {
    if !x.is_finite() {
        return "—".to_string();
    }
    if x == 0.0 {
        return "0".to_string();
    }
    if x >= 1000.0 {
        format!("{x:.2}")
    } else if x >= 1.0 {
        format!("{x:.4}")
    } else {
        format!("{x:.6}")
    }
}

/// Renders a wide dark card matching the standard signal layout (Turkish labels).
///
/// Wraps the plotters call in `catch_unwind` so a missing font (which plotters
/// turns into a hard panic) degrades to an `Err` instead of killing a tokio worker thread.
pub fn try_render_ai_approval_card_png(
    input: &AiApprovalCardInput,
) -> Result<Vec<u8>, SignalCardRenderError> {
    let input = input.clone();
    match std::panic::catch_unwind(move || render_ai_approval_inner(&input)) {
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

fn render_ai_approval_inner(
    input: &AiApprovalCardInput,
) -> Result<Vec<u8>, SignalCardRenderError> {
    let ref_px = input.reference_price;
    if !ref_px.is_finite() || ref_px.abs() < 1e-12 {
        return Err(SignalCardRenderError::Plotters(
            "reference_price not finite".into(),
        ));
    }
    let durum = if input.flat_no_trade {
        "FLAT"
    } else if input.side_long {
        "LONG"
    } else {
        "SHORT"
    };
    let strength = (input.confidence_0_1 * 10.0).round().clamp(1.0, 10.0) as u8;
    let strength_word = strength_label_tr(strength).to_string();
    let chg = input.approx_change_pct.unwrap_or(0.0);
    let tri = if chg >= 0.0 { "▲" } else { "▼" };
    let price_caption = format!(
        "{}  {} {:+.2}%",
        format_compact_price(input.last_close),
        tri,
        chg
    );
    let entry_line = format_compact_price(ref_px);
    let (stop_line, tp_line) = if input.flat_no_trade {
        (
            "— (yönlü işlem yok)".to_string(),
            "— (yönlü işlem yok)".to_string(),
        )
    } else {
        let sl_pct_label = (input.stop_loss - ref_px) / ref_px * 100.0;
        let tp_pct_label = (input.take_profit - ref_px) / ref_px * 100.0;
        (
            format!(
                "{} ({:+.2}%)",
                format_compact_price(input.stop_loss),
                sl_pct_label
            ),
            format!(
                "{} ({:+.2}%)",
                format_compact_price(input.take_profit),
                tp_pct_label
            ),
        )
    };
    let header_left = format!("{} ({})", input.symbol.trim().to_uppercase(), input.timeframe);
    let sub = format!(
        "AI güven {:.0}% · {}",
        (input.confidence_0_1 * 100.0).round(),
        durum
    );
    let params = SignalCardParams {
        header_left,
        header_right: "AI ONAY".to_string(),
        price_caption,
        price_line_positive: chg >= 0.0,
        side_label: durum.to_string(),
        strength_10: strength,
        strength_word,
        subscores_line: sub,
        entry_line,
        stop_line,
        tp_line,
        canvas_width: AI_CARD_W,
        canvas_height: AI_CARD_H,
    };
    render_signal_card_png(&params)
}
