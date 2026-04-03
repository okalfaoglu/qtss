//! Pine `basechartpatterns.getPatternNameById` / `patternType` 1–13 — aynı sıra ve metinler.
//!
//! `strum` bilinçli olarak kullanılmıyor (crate `Cargo.toml`’da yok). `Display` ve kimlik
//! eşlemesi [`PatternId::from_repr`], [`pattern_name_by_id`] ve [`std::fmt::Display`] ile
//! yapılır; ileride `strum` derive eklerseniz bağımlılığı ekleyin veya manuel impl’i koruyun.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Pine’deki `patternType` (0 = geçersiz / filtrelendi).
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
    DoubleTop = 14,
    DoubleBottom = 15,
    HeadAndShoulders = 16,
    InverseHeadAndShoulders = 17,
    TripleTop = 18,
    TripleBottom = 19,
    BullishFlag = 20,
    BearishFlag = 21,
}

impl PatternId {
    #[must_use]
    pub fn from_repr(v: i32) -> Option<Self> {
        match v {
            1 => Some(Self::AscendingChannel),
            2 => Some(Self::DescendingChannel),
            3 => Some(Self::RangingChannel),
            4 => Some(Self::RisingWedgeExpanding),
            5 => Some(Self::FallingWedgeExpanding),
            6 => Some(Self::DivergingTriangle),
            7 => Some(Self::AscendingTriangleExpanding),
            8 => Some(Self::DescendingTriangleExpanding),
            9 => Some(Self::RisingWedgeContracting),
            10 => Some(Self::FallingWedgeContracting),
            11 => Some(Self::ConvergingTriangle),
            12 => Some(Self::DescendingTriangleContracting),
            13 => Some(Self::AscendingTriangleContracting),
            14 => Some(Self::DoubleTop),
            15 => Some(Self::DoubleBottom),
            16 => Some(Self::HeadAndShoulders),
            17 => Some(Self::InverseHeadAndShoulders),
            18 => Some(Self::TripleTop),
            19 => Some(Self::TripleBottom),
            20 => Some(Self::BullishFlag),
            21 => Some(Self::BearishFlag),
            _ => None,
        }
    }
}

impl fmt::Display for PatternId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(pattern_name_by_id(*self as i32))
    }
}

/// Pine `getPatternNameById(id)` ile aynı metinler.
#[must_use]
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
        14 => "Double Top",
        15 => "Double Bottom",
        16 => "Head and Shoulders",
        17 => "Inverse Head and Shoulders",
        18 => "Triple Top",
        19 => "Triple Bottom",
        20 => "Bullish Flag",
        21 => "Bearish Flag",
        _ => "Error",
    }
}
