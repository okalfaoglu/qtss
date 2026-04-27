//! Phase A-E state machine. Consumers feed it events in chronological
//! order; reading `phase()` / `bias()` gives the current Wyckoff
//! phase for the range.
//!
//! Simple rules (Pruden / Weis convention):
//!   * Phase A — SC, AR arrive (stopping action)
//!   * Phase B — multiple STs (cause-building)
//!   * Phase C — Spring (or UTAD)
//!   * Phase D — SOS + LPS (direction reveals)
//!   * Phase E — BU + trend

use crate::event::{WyckoffEvent, WyckoffEventKind};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WyckoffPhase {
    None,
    A,
    B,
    C,
    D,
    E,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WyckoffBias {
    Neutral,
    Accumulation,
    Distribution,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WyckoffPhaseTracker {
    pub phase: WyckoffPhase,
    pub bias: WyckoffBias,
    pub saw_ps_or_bc_or_sc: bool,
    pub st_count: u32,
    pub saw_spring_or_utad: bool,
    pub saw_sos_or_sow: bool,
    pub saw_lps: bool,
    pub saw_bu: bool,
}

impl Default for WyckoffPhase {
    fn default() -> Self {
        WyckoffPhase::None
    }
}
impl Default for WyckoffBias {
    fn default() -> Self {
        WyckoffBias::Neutral
    }
}

impl WyckoffPhaseTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one event — updates `phase` / `bias` fields.
    pub fn feed(&mut self, ev: &WyckoffEvent) {
        if ev.kind.is_accumulation() && self.bias == WyckoffBias::Neutral {
            self.bias = WyckoffBias::Accumulation;
        } else if ev.kind.is_distribution() && self.bias == WyckoffBias::Neutral {
            self.bias = WyckoffBias::Distribution;
        }
        match ev.kind {
            WyckoffEventKind::Ps
            | WyckoffEventKind::Sc
            | WyckoffEventKind::Bc
            | WyckoffEventKind::Ar => {
                self.saw_ps_or_bc_or_sc = true;
                if self.phase == WyckoffPhase::None {
                    self.phase = WyckoffPhase::A;
                }
            }
            WyckoffEventKind::St => {
                self.st_count += 1;
                if self.phase == WyckoffPhase::A {
                    self.phase = WyckoffPhase::B;
                }
            }
            WyckoffEventKind::Spring | WyckoffEventKind::Utad => {
                self.saw_spring_or_utad = true;
                self.phase = WyckoffPhase::C;
            }
            WyckoffEventKind::Sos | WyckoffEventKind::Sow => {
                self.saw_sos_or_sow = true;
                if self.phase == WyckoffPhase::C || self.phase == WyckoffPhase::B {
                    self.phase = WyckoffPhase::D;
                }
            }
            WyckoffEventKind::Lps => {
                self.saw_lps = true;
                if self.phase == WyckoffPhase::D {
                    self.phase = WyckoffPhase::D; // stays D until BU
                }
            }
            WyckoffEventKind::Bu => {
                self.saw_bu = true;
                self.phase = WyckoffPhase::E;
            }
            // Phase B intra-range probes — neither advance nor retreat
            // the phase tracker. They confirm the cause-building
            // motion but the actual structural pivot is Spring/UTAD
            // (handled above).
            WyckoffEventKind::Ut | WyckoffEventKind::Msow => {
                if self.phase == WyckoffPhase::A
                    || self.phase == WyckoffPhase::B
                {
                    self.phase = WyckoffPhase::B;
                }
            }
            WyckoffEventKind::Test => {}
        }
    }

    pub fn phase(&self) -> WyckoffPhase {
        self.phase
    }
    pub fn bias(&self) -> WyckoffBias {
        self.bias
    }
}
