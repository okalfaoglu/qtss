//! Multi-timeframe confirmation.

use crate::corrective::CorrectiveWave;
use crate::motive::MotiveWave;

#[derive(Debug, Clone)]
pub struct MTFConfirmation {
    pub signal_valid: bool,
    pub confirm_aligned: bool,
    pub bias_aligned: bool,
    pub confidence: f64,
    pub reason: String,
}

pub fn check_motive_mtf(
    signal_motive: &MotiveWave,
    confirm_motive: Option<&MotiveWave>,
    bias_trend: i8,
) -> MTFConfirmation {
    let mut confidence = 0.5;
    let mut reasons = vec!["signal_valid"];

    let confirm_aligned = match confirm_motive {
        Some(cm) => cm.direction == signal_motive.direction,
        None => false,
    };
    if confirm_aligned { confidence += 0.25; reasons.push("confirm_aligned"); }

    let bias_aligned = bias_trend == signal_motive.direction && bias_trend != 0;
    if bias_aligned { confidence += 0.25; reasons.push("bias_aligned"); }

    MTFConfirmation {
        signal_valid: true,
        confirm_aligned,
        bias_aligned,
        confidence,
        reason: reasons.join("; "),
    }
}

pub fn check_corrective_mtf(
    signal_corr: &CorrectiveWave,
    confirm_corr: Option<&CorrectiveWave>,
    bias_trend: i8,
) -> MTFConfirmation {
    let mut confidence = 0.5;
    let mut reasons = vec!["signal_valid"];

    let confirm_aligned = match confirm_corr {
        Some(cc) => cc.direction == signal_corr.direction || cc.direction == -signal_corr.direction,
        None => false,
    };
    if confirm_aligned { confidence += 0.25; reasons.push("confirm_aligned"); }

    let bias_aligned = bias_trend == -signal_corr.direction && bias_trend != 0;
    if bias_aligned { confidence += 0.25; reasons.push("bias_opposite"); }

    MTFConfirmation {
        signal_valid: true,
        confirm_aligned,
        bias_aligned,
        confidence,
        reason: reasons.join("; "),
    }
}
