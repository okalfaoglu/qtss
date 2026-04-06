//! Telegram-friendly PNG cards for multi-timeframe signal snapshots.
//!
//! Rendering uses plotters `BitMapBackend` + a system/embedded sans font (Turkish labels).
//! Install `fonts-dejavu-core` (or set `QTSS_SIGNAL_CARD_FONT_FAMILY`) on minimal servers.

mod ai_approval;
mod render;
mod subscores;

pub use ai_approval::{
    format_compact_price, try_render_ai_approval_card_png, AiApprovalCardInput,
};
pub use render::{render_signal_card_png, SignalCardParams, SignalCardRenderError};
pub use subscores::{strength_label_tr, subscores_tmr};
