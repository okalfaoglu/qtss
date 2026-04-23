use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidatorVerdict {
    Hold,
    Invalidate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvalidationReason {
    GeometryBroken,
    ZoneFilled,
    GapClosed,
    ReEntryFakeout,
    StructuralBreak,
    TimeExpired,
    OtherFamilyRule,
}

impl InvalidationReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GeometryBroken => "geometry_broken",
            Self::ZoneFilled => "zone_filled",
            Self::GapClosed => "gap_closed",
            Self::ReEntryFakeout => "re_entry_fakeout",
            Self::StructuralBreak => "structural_break",
            Self::TimeExpired => "time_expired",
            Self::OtherFamilyRule => "other_family_rule",
        }
    }
}
