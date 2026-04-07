//! Telegram-friendly PNG cards for multi-timeframe signal snapshots.
//!
//! Text uses plotters `ttf` + [`embedded_font`] — DejaVu Sans bytes are bundled at compile time
//! (see `assets/DejaVuSans.ttf`, fetched by `build.rs` if missing). No system fontconfig required.

mod ai_approval;
mod embedded_font;
mod operational_approval;
mod render;
mod subscores;

pub use ai_approval::{
    format_compact_price, try_render_ai_approval_card_png, AiApprovalCardInput,
};
pub use operational_approval::{
    try_render_operational_approval_card_png, OperationalApprovalCardInput,
};
pub use render::{render_signal_card_png, SignalCardParams, SignalCardRenderError};
pub use subscores::{strength_label_tr, subscores_tmr};
