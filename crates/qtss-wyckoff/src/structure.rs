//! Wyckoff Structure State Machine — tracks phase progression (A→B→C→D→E)
//! and structure type (Accumulation/Distribution/Re-accumulation/Re-distribution).
//!
//! The tracker collects events emitted by the detector and maintains a
//! running assessment of the current phase, schematic type, and key levels
//! (creek, ice, range).

use serde::{Deserialize, Serialize};

// =========================================================================
// Types
// =========================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WyckoffSchematic {
    Accumulation,
    Distribution,
    ReAccumulation,
    ReDistribution,
}

impl WyckoffSchematic {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Accumulation => "accumulation",
            Self::Distribution => "distribution",
            Self::ReAccumulation => "reaccumulation",
            Self::ReDistribution => "redistribution",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum WyckoffPhase {
    A,
    B,
    C,
    D,
    E,
}

impl WyckoffPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::A => "A",
            Self::B => "B",
            Self::C => "C",
            Self::D => "D",
            Self::E => "E",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WyckoffEvent {
    // Phase A
    PS,
    SC,
    BC,
    AR,
    ST,
    // Phase B
    UA,
    STB,
    // Phase C
    Spring,
    UTAD,
    Shakeout,
    // Phase D
    SOS,
    SOW,
    LPS,
    LPSY,
    JAC,
    BreakOfIce,
    BUEC,
    // Misc
    SOT,
    Markup,
    Markdown,
}

impl WyckoffEvent {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PS => "PS",
            Self::SC => "SC",
            Self::BC => "BC",
            Self::AR => "AR",
            Self::ST => "ST",
            Self::UA => "UA",
            Self::STB => "ST-B",
            Self::Spring => "Spring",
            Self::UTAD => "UTAD",
            Self::Shakeout => "Shakeout",
            Self::SOS => "SOS",
            Self::SOW => "SOW",
            Self::LPS => "LPS",
            Self::LPSY => "LPSY",
            Self::JAC => "JAC",
            Self::BreakOfIce => "BreakOfIce",
            Self::BUEC => "BUEC",
            Self::SOT => "SOT",
            Self::Markup => "Markup",
            Self::Markdown => "Markdown",
        }
    }

    /// Determine which phase this event belongs to.
    pub fn phase(self) -> WyckoffPhase {
        match self {
            Self::PS | Self::SC | Self::BC | Self::AR | Self::ST => WyckoffPhase::A,
            Self::UA | Self::STB => WyckoffPhase::B,
            Self::Spring | Self::UTAD | Self::Shakeout => WyckoffPhase::C,
            Self::SOS | Self::SOW | Self::LPS | Self::LPSY
            | Self::JAC | Self::BreakOfIce | Self::BUEC | Self::SOT => WyckoffPhase::D,
            Self::Markup | Self::Markdown => WyckoffPhase::E,
        }
    }
}

// =========================================================================
// Recorded event
// =========================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordedEvent {
    pub event: WyckoffEvent,
    pub bar_index: u64,
    pub price: f64,
    pub score: f64,
}

// =========================================================================
// Structure Tracker
// =========================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WyckoffStructureTracker {
    pub schematic: WyckoffSchematic,
    pub current_phase: WyckoffPhase,
    pub events: Vec<RecordedEvent>,
    pub range_top: f64,
    pub range_bottom: f64,
    pub creek: Option<f64>,
    pub ice: Option<f64>,
    pub slope_deg: f64,
    pub is_active: bool,
    pub failure_reason: Option<String>,
}

impl WyckoffStructureTracker {
    /// Start a new structure from a detected trading range.
    pub fn new(schematic: WyckoffSchematic, range_top: f64, range_bottom: f64) -> Self {
        Self {
            schematic,
            current_phase: WyckoffPhase::A,
            events: Vec::new(),
            range_top,
            range_bottom,
            creek: None,
            ice: None,
            slope_deg: 0.0,
            is_active: true,
            failure_reason: None,
        }
    }

    /// Record a new event and advance phase if warranted.
    pub fn record_event(&mut self, event: WyckoffEvent, bar_index: u64, price: f64, score: f64) {
        self.events.push(RecordedEvent {
            event,
            bar_index,
            price,
            score,
        });

        // Update key levels
        match event {
            WyckoffEvent::SC => {
                self.range_bottom = self.range_bottom.min(price);
            }
            WyckoffEvent::BC => {
                self.range_top = self.range_top.max(price);
            }
            WyckoffEvent::AR => {
                match self.schematic {
                    WyckoffSchematic::Accumulation | WyckoffSchematic::ReAccumulation => {
                        self.range_top = self.range_top.max(price);
                        self.creek = Some(price);
                    }
                    WyckoffSchematic::Distribution | WyckoffSchematic::ReDistribution => {
                        self.range_bottom = self.range_bottom.min(price);
                        self.ice = Some(price);
                    }
                }
            }
            WyckoffEvent::JAC => {
                self.creek = Some(price);
            }
            WyckoffEvent::BreakOfIce => {
                self.ice = Some(price);
            }
            _ => {}
        }

        // Phase advancement
        self.try_advance_phase();
    }

    /// Try to advance phase based on accumulated events.
    fn try_advance_phase(&mut self) {
        let phase_events: Vec<WyckoffEvent> = self.events.iter().map(|e| e.event).collect();

        match self.current_phase {
            WyckoffPhase::A => {
                // A → B requires: SC/BC + AR + ST
                let has_climax = phase_events.contains(&WyckoffEvent::SC)
                    || phase_events.contains(&WyckoffEvent::BC);
                let has_ar = phase_events.contains(&WyckoffEvent::AR);
                let has_st = phase_events.contains(&WyckoffEvent::ST);
                if has_climax && has_ar && has_st {
                    self.current_phase = WyckoffPhase::B;
                }
            }
            WyckoffPhase::B => {
                // B → C requires: time in range (≥2 events in Phase B)
                let b_count = phase_events.iter()
                    .filter(|e| e.phase() == WyckoffPhase::B)
                    .count();
                // Or any Phase C event triggers the transition
                let has_c_event = phase_events.iter().any(|e| e.phase() == WyckoffPhase::C);
                if b_count >= 2 || has_c_event {
                    self.current_phase = WyckoffPhase::C;
                }
            }
            WyckoffPhase::C => {
                // C → D requires: Spring/UTAD/Shakeout + subsequent test
                let has_spring = phase_events.contains(&WyckoffEvent::Spring)
                    || phase_events.contains(&WyckoffEvent::Shakeout);
                let has_utad = phase_events.contains(&WyckoffEvent::UTAD);
                let has_d_event = phase_events.iter().any(|e| e.phase() == WyckoffPhase::D);
                if (has_spring || has_utad) && has_d_event {
                    self.current_phase = WyckoffPhase::D;
                }
            }
            WyckoffPhase::D => {
                // D → E requires: JAC + BUEC (accumulation) or BreakOfIce (distribution)
                let has_jac = phase_events.contains(&WyckoffEvent::JAC);
                let has_boi = phase_events.contains(&WyckoffEvent::BreakOfIce);
                let has_markup = phase_events.contains(&WyckoffEvent::Markup);
                let has_markdown = phase_events.contains(&WyckoffEvent::Markdown);
                if has_jac || has_boi || has_markup || has_markdown {
                    self.current_phase = WyckoffPhase::E;
                }
            }
            WyckoffPhase::E => {
                // Terminal phase — structure is complete
            }
        }
    }

    /// Mark the structure as failed (e.g. accumulation turns into distribution).
    pub fn fail(&mut self, reason: &str) {
        self.is_active = false;
        self.failure_reason = Some(reason.to_string());
    }

    /// Reclassify the structure (e.g. failed accumulation → re-distribution).
    pub fn reclassify(&mut self, new_schematic: WyckoffSchematic) {
        self.schematic = new_schematic;
    }

    /// Confidence estimate based on events collected and their scores.
    pub fn confidence(&self) -> f64 {
        if self.events.is_empty() {
            return 0.0;
        }
        let total_score: f64 = self.events.iter().map(|e| e.score).sum();
        let avg = total_score / self.events.len() as f64;
        // Bonus for later phases (more confirmed)
        let phase_bonus = match self.current_phase {
            WyckoffPhase::A => 0.0,
            WyckoffPhase::B => 0.05,
            WyckoffPhase::C => 0.10,
            WyckoffPhase::D => 0.15,
            WyckoffPhase::E => 0.20,
        };
        (avg + phase_bonus).min(1.0)
    }

    /// Map event name from detector to WyckoffEvent.
    pub fn event_from_detector_name(name: &str) -> Option<WyckoffEvent> {
        match name {
            "selling_climax" => Some(WyckoffEvent::SC),
            "buying_climax" => Some(WyckoffEvent::BC),
            "automatic_rally" => Some(WyckoffEvent::AR),
            "automatic_reaction" => Some(WyckoffEvent::AR),
            "secondary_test" => Some(WyckoffEvent::ST),
            "spring" => Some(WyckoffEvent::Spring),
            "upthrust" => Some(WyckoffEvent::UTAD),
            "upthrust_action" => Some(WyckoffEvent::UA),
            "shakeout" => Some(WyckoffEvent::Shakeout),
            "sign_of_strength" => Some(WyckoffEvent::SOS),
            "sign_of_weakness" => Some(WyckoffEvent::SOW),
            "last_point_of_support" => Some(WyckoffEvent::LPS),
            "last_point_of_supply" => Some(WyckoffEvent::LPSY),
            "jump_across_creek" => Some(WyckoffEvent::JAC),
            "break_of_ice" => Some(WyckoffEvent::BreakOfIce),
            "shortening_of_thrust" => Some(WyckoffEvent::SOT),
            _ => None,
        }
    }
}
