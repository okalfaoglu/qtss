//! Embedded DejaVu Sans so PNG cards work without fontconfig / system fonts (headless servers).

use plotters::style::{register_font, FontStyle};
use std::sync::Once;

static REGISTER_EMBEDDED_FONT: Once = Once::new();

/// Registers the bundled TTF once; safe to call from every render path.
pub fn ensure_dejavu_sans_registered() {
    REGISTER_EMBEDDED_FONT.call_once(|| {
        register_font(
            "DejaVu Sans",
            FontStyle::Normal,
            include_bytes!("../assets/DejaVuSans.ttf"),
        )
        .unwrap_or_else(|_e| {
            panic!("qtss-signal-card: embedded DejaVuSans.ttf failed to register");
        });
    });
}
