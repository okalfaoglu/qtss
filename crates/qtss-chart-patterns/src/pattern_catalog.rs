//! Pine `basechartpatterns.getPatternNameById` / `patternType` 1–13 — aynı sıra ve metinler.

use serde::{Deserialize, Serialize};
use strum::{Display, EnumIter, FromRepr};

/// Pine’deki `patternType` (0 = geçersiz / filtrelendi).
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Display, EnumIter, FromRepr)]
pub enum PatternId {
    AscendingChannel = 1,
    DescendingChannel = 2,
    RangingChannel = 3,
    RisingWedgeExpanding = 4,
    FallingWedgeExpanding = 5,
    DivergingTriangle = 6,
    AscendingTriangleExpanding = 7,
    DescendingTriangleExpanding = 8,
    RisingWedgeContracting = 9,
    FallingWedgeContracting = 10,
    ConvergingTriangle = 11,
    DescendingTriangleContracting = 12,
    AscendingTriangleContracting = 13,
}

/// Pine `getPatternNameById(id)` ile aynı metinler.
pub fn pattern_name_by_id(id: i32) -> &'static str {
    match id {
        1 => "Ascending Channel",
        2 => "Descending Channel",
        3 => "Ranging Channel",
        4 => "Rising Wedge (Expanding)",
        5 => "Falling Wedge (Expanding)",
        6 => "Diverging Triangle",
        7 => "Ascending Triangle (Expanding)",
        8 => "Descending Triangle (Expanding)",
        9 => "Rising Wedge (Contracting)",
        10 => "Falling Wedge (Contracting)",
        11 => "Converging Triangle",
        12 => "Descending Triangle (Contracting)",
        13 => "Ascending Triangle (Contracting)",
        _ => "Error",
    }
}
