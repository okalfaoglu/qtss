//! Trendoscope **Pine v6** `Trendoscope/utils` — `export enum Theme` + `export method getColors(Theme)`.
//!
//! - **DARK / LIGHT** aşağıdaki sabitlerle **bayt bazında aynı** (`array.from(color.rgb(...))` sırası).
//! - **CUSTOM**: göstergedeki `useCustomColors` + `customColorsArray`; sunucu `PatternDrawingBatch` için yalnızca
//!   `theme_dark` + bu tablolar (veya ileride istemci özel hex) kullanılır — `utils` içindeki CUSTOM dalı ayrı.
//! - `check_overflow`, `get_trend_series`, `timer`, `watermark`, `runTimer` gibi yardımcılar bu crate’te ayrı sembol
//!   olarak yok; çizgi fiyatı `line_price_at_bar_index`, teğet/ skor mantığı `trend_line_inspect` / `inspect_*` ile kapsanır.

/// Dark tema: Pine `getColors(Theme.DARK)` — **22** renk (ACP çizim indeksi `(pattern_type_id - 1) % len`).
pub const THEME_DARK_RGB: &[(u8, u8, u8); 22] = &[
    (251, 244, 109),
    (141, 186, 81),
    (74, 159, 245),
    (255, 153, 140),
    (255, 149, 0),
    (0, 234, 211),
    (167, 153, 183),
    (255, 210, 113),
    (119, 217, 112),
    (95, 129, 228),
    (235, 146, 190),
    (198, 139, 89),
    (200, 149, 149),
    (196, 182, 182),
    (255, 190, 15),
    (192, 226, 24),
    (153, 140, 235),
    (206, 31, 107),
    (251, 54, 64),
    (194, 255, 217),
    (255, 219, 197),
    (121, 180, 183),
];

/// Light tema: Pine `getColors(Theme.LIGHT)` — **21** renk (aynı mod işlemi).
pub const THEME_LIGHT_RGB: &[(u8, u8, u8); 21] = &[
    (61, 86, 178),
    (57, 163, 136),
    (250, 30, 14),
    (169, 51, 58),
    (225, 87, 138),
    (62, 124, 23),
    (244, 164, 66),
    (134, 72, 121),
    (113, 159, 176),
    (170, 46, 230),
    (161, 37, 104),
    (189, 32, 0),
    (16, 86, 82),
    (200, 92, 92),
    (63, 51, 81),
    (114, 106, 149),
    (171, 109, 35),
    (247, 136, 18),
    (51, 71, 86),
    (12, 123, 147),
    (195, 43, 173),
];

#[must_use]
pub fn rgb_to_hex(r: u8, g: u8, b: u8) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pine `utils` `getColors(Theme.DARK)` ilk/son öğe ve uzunluk (regresyon).
    #[test]
    fn theme_dark_matches_trendoscope_utils_v6() {
        assert_eq!(THEME_DARK_RGB.len(), 22);
        assert_eq!(THEME_DARK_RGB[0], (251, 244, 109));
        assert_eq!(THEME_DARK_RGB[21], (121, 180, 183));
    }

    /// Pine `utils` `getColors(Theme.LIGHT)` ilk/son öğe ve uzunluk.
    #[test]
    fn theme_light_matches_trendoscope_utils_v6() {
        assert_eq!(THEME_LIGHT_RGB.len(), 21);
        assert_eq!(THEME_LIGHT_RGB[0], (61, 86, 178));
        assert_eq!(THEME_LIGHT_RGB[20], (195, 43, 173));
    }
}
