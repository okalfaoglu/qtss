//! Position strength (0–10) trend and entry scenario — see `docs/SIGNAL_POSITION_SCORE_RULES.md`.

/// Last-three-samples classification (`insufficient_history` until three values exist).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScoreTrendKind {
    InsufficientHistory,
    Stable,
    Improving,
    Worsening,
    RapidDecline,
    FreeFall,
}

impl ScoreTrendKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InsufficientHistory => "insufficient_history",
            Self::Stable => "stable",
            Self::Improving => "improving",
            Self::Worsening => "worsening",
            Self::RapidDecline => "rapid_decline",
            Self::FreeFall => "free_fall",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScoreTrendOutcome {
    pub kind: ScoreTrendKind,
    /// Stable English token for i18n / automation (`ease_toward_tp`, `tighten_stop`, …).
    pub action: &'static str,
}

/// Entry-vs-current label when LONG/SHORT is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PositionScenarioKind {
    None,
    StrengtheningExcellent,
    StableGood,
    DangerReversal,
    MomentumFading,
}

impl PositionScenarioKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::StrengtheningExcellent => "strengthening_excellent",
            Self::StableGood => "stable_good",
            Self::DangerReversal => "danger_reversal",
            Self::MomentumFading => "momentum_fading",
        }
    }
}

/// Roll `[…, t-2, t-1]` from the previous payload plus current (post-confluence) strength.
#[must_use]
pub fn roll_position_strength_history(prev: Option<&[u8]>, current: u8) -> Vec<u8> {
    let c = current.min(10);
    match prev {
        None | Some([]) => vec![c],
        Some(xs) if xs.len() == 1 => vec![xs[0], c],
        Some(xs) => {
            let a = xs[xs.len().saturating_sub(2)];
            let b = *xs.last().unwrap_or(&c);
            vec![a, b, c]
        }
    }
}

#[must_use]
pub fn classify_score_trend(history: &[u8]) -> ScoreTrendOutcome {
    if history.len() < 3 {
        return ScoreTrendOutcome {
            kind: ScoreTrendKind::InsufficientHistory,
            action: "insufficient_history",
        };
    }
    let n = history.len();
    let tri = [history[n - 3], history[n - 2], history[n - 1]];
    classify_score_trend_triple(tri)
}

#[must_use]
pub fn classify_score_trend_triple(s: [u8; 3]) -> ScoreTrendOutcome {
    let (a, b, c) = (s[0].min(10), s[1].min(10), s[2].min(10));

    if a == b && b == c {
        return ScoreTrendOutcome {
            kind: ScoreTrendKind::Stable,
            action: "watch_no_issue",
        };
    }

    // Table: 5 → 3 → 2 (free fall).
    if a >= 5 && c <= 2 && a > b && b > c && a.saturating_sub(c) >= 3 {
        return ScoreTrendOutcome {
            kind: ScoreTrendKind::FreeFall,
            action: "act_immediately",
        };
    }

    // Table: 6 → 4 → 3 (rapid decline).
    if a >= 6 && c <= 3 && a > b && b > c {
        return ScoreTrendOutcome {
            kind: ScoreTrendKind::RapidDecline,
            action: "plan_exit_or_wait_sl",
        };
    }

    // Table: 6 → 7 → 8 (strict improvement).
    if c > b && b > a {
        return ScoreTrendOutcome {
            kind: ScoreTrendKind::Improving,
            action: "ease_toward_tp",
        };
    }

    // Table: 8 → 6 → 5 (strict worsening).
    if c < b && b < a {
        return ScoreTrendOutcome {
            kind: ScoreTrendKind::Worsening,
            action: "tighten_stop",
        };
    }

    if c > a && c >= b {
        return ScoreTrendOutcome {
            kind: ScoreTrendKind::Improving,
            action: "ease_toward_tp",
        };
    }
    if c < a && c <= b {
        return ScoreTrendOutcome {
            kind: ScoreTrendKind::Worsening,
            action: "tighten_stop",
        };
    }

    ScoreTrendOutcome {
        kind: ScoreTrendKind::Stable,
        action: "watch_no_issue",
    }
}

#[must_use]
pub fn classify_position_scenario(entry: u8, current: u8) -> PositionScenarioKind {
    let e = entry.min(10);
    let cur = current.min(10);

    if e >= 9 && cur <= 6 && cur < e {
        return PositionScenarioKind::MomentumFading;
    }
    if e >= 7 && cur <= 5 && cur < e {
        return PositionScenarioKind::DangerReversal;
    }
    if cur > e && cur >= 8 {
        return PositionScenarioKind::StrengtheningExcellent;
    }
    if cur == e && e >= 8 {
        return PositionScenarioKind::StableGood;
    }
    PositionScenarioKind::None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trend_table_examples() {
        let t = classify_score_trend_triple([6, 7, 8]);
        assert_eq!(t.kind, ScoreTrendKind::Improving);
        assert_eq!(t.action, "ease_toward_tp");

        let t = classify_score_trend_triple([7, 7, 7]);
        assert_eq!(t.kind, ScoreTrendKind::Stable);

        let t = classify_score_trend_triple([8, 6, 5]);
        assert_eq!(t.kind, ScoreTrendKind::Worsening);
        assert_eq!(t.action, "tighten_stop");

        let t = classify_score_trend_triple([6, 4, 3]);
        assert_eq!(t.kind, ScoreTrendKind::RapidDecline);

        let t = classify_score_trend_triple([5, 3, 2]);
        assert_eq!(t.kind, ScoreTrendKind::FreeFall);
    }

    #[test]
    fn trend_insufficient_until_three() {
        let t = classify_score_trend(&[6, 7]);
        assert_eq!(t.kind, ScoreTrendKind::InsufficientHistory);
    }

    #[test]
    fn roll_history_chains() {
        assert_eq!(roll_position_strength_history(None, 6), vec![6]);
        assert_eq!(roll_position_strength_history(Some(&[6]), 7), vec![6, 7]);
        assert_eq!(
            roll_position_strength_history(Some(&[6, 7]), 8),
            vec![6, 7, 8]
        );
        assert_eq!(
            roll_position_strength_history(Some(&[6, 7, 8]), 9),
            vec![7, 8, 9]
        );
    }

    #[test]
    fn scenario_table_examples() {
        assert_eq!(
            classify_position_scenario(7, 9),
            PositionScenarioKind::StrengtheningExcellent
        );
        assert_eq!(
            classify_position_scenario(8, 8),
            PositionScenarioKind::StableGood
        );
        assert_eq!(
            classify_position_scenario(7, 4),
            PositionScenarioKind::DangerReversal
        );
        assert_eq!(
            classify_position_scenario(9, 6),
            PositionScenarioKind::MomentumFading
        );
    }
}
