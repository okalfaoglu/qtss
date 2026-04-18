//! Detector configuration.

use crate::error::{ClassicalError, ClassicalResult};
use qtss_domain::v2::pivot::PivotLevel;

#[derive(Debug, Clone)]
pub struct ClassicalConfig {
    /// Which pivot level to consume.
    pub pivot_level: PivotLevel,
    /// Drop candidates whose structural score falls under this floor.
    pub min_structural_score: f32,
    /// Maximum allowed relative deviation between the two "equal" peaks
    /// of a double-top / shoulders / triangle base, expressed as a
    /// fraction of the price (0.03 = 3%).
    pub equality_tolerance: f64,
    /// Triangle convergence: the apex (intersection of upper/lower
    /// trendlines) must be within `apex_horizon` future bars from the
    /// last pivot, otherwise the pattern is too loose.
    pub apex_horizon_bars: u64,
    /// Slope threshold for "effectively flat" trendline in triangles,
    /// as a fraction of reference price per bar. Gaussian fall-off begins
    /// beyond this value. Default 0.001 = 0.1%/bar. Per CLAUDE.md #2 this
    /// was hardcoded in shapes.rs; now configurable per TF/asset class.
    pub flatness_threshold_pct: f64,
    /// Minimum flatness score required to keep an asc/desc triangle
    /// candidate. Below this score the "flat" side is too sloped to
    /// qualify. Default 0.3.
    pub flatness_min_score: f64,
    /// Neckline equality tolerance multiplier for H&S / Inverse H&S.
    /// The neckline has looser symmetry than the shoulders so we
    /// multiply `equality_tolerance` by this. Default 1.5. Also used as
    /// an upper bound for acceptable neckline slope |N2-N1|/mid —
    /// beyond this, the neckline is too steep for a valid H&S.
    pub neckline_tolerance_mult: f64,
    /// Symmetrical-triangle slope symmetry tolerance (how close
    /// |upper.slope| must be to lower.slope as a fraction of their
    /// mean). Default 0.5.
    pub triangle_symmetry_tol: f64,
    /// P3 — H&S neckline slope cap, as fraction of neckline midpoint
    /// per bar. Kitabi kural (Bulkowski): neckline ±5° kabul edilir.
    /// Daha dik eğim → farklı bir formasyon. Default 0.003 (0.3%/bar).
    /// Pattern reddedilir eğim bunu aşarsa.
    pub hs_max_neckline_slope_pct: f64,
    /// P3 — H&S omuz ZAMAN simetrisi toleransı. |LS→H bars - H→RS bars|
    /// / avg ≤ cap olmalı. Bulkowski & Edwards/Magee: "ideal H&S'de
    /// omuzlar zaman olarak kabaca eşittir". Score lineer düşer, bu
    /// orandan sonra 0'a gider. Default 0.5 (±50% tolerans).
    pub hs_time_symmetry_tol: f64,
    /// P5.1 — Rectangle maksimum trendline eğimi (|slope|/ref_price, per
    /// bar). Üst ve alt bantlar bu eşiğin altında kalmalı; aksi halde
    /// "channel" veya "triangle" adayıdır. Default 0.002 (0.2%/bar).
    pub rectangle_max_slope_pct: f64,
    /// P5.1 — Rectangle minimum süresi (ilk pivot ile son pivot arası
    /// bar sayısı). Kısa "range" gürültüsünü eler. Default 15.
    pub rectangle_min_bars: u64,
    /// P5.2 — Flagpole için minimum yönlü hareket, ATR çarpanı olarak.
    /// Flagpole = flag gövdesinden önce gelen güçlü momentum hareketi.
    /// Default 3.0 (hareket ≥ 3×ATR olmalı).
    pub flag_pole_min_move_atr: f64,
    /// P5.2 — Flagpole'un geriye doğru bakılacak maksimum bar sayısı.
    /// Bu pencere içinde ATR hesaplanır ve ekstrem close aranır.
    /// Default 20.
    pub flag_pole_max_bars: u64,
    /// P5.2 — Flag gövdesinin flagpole yüksekliğine oranı üst sınırı.
    /// Bulkowski: flag retrace'i flagpole'un < %50'si olmalı. Default 0.5.
    pub flag_max_retrace_pct: f64,
    /// P5.2 — Flag / Pennant ATR pencere periyodu (Wilder ATR).
    /// Default 14.
    pub flag_atr_period: u64,
    /// P5.2 — Flag kanal paralellik toleransı; |upper.slope - lower.slope|
    /// / avg < tol olmalı. Default 0.3 (±30%).
    pub flag_parallelism_tol: f64,
    /// P5.2 — Pennant (küçük simetrik üçgen) maksimum yüksekliğinin
    /// flagpole'a oranı. Default 0.4 (%40).
    pub pennant_max_height_pct_of_pole: f64,
    /// P5.4 — Channel paralellik toleransı; |upper.slope - lower.slope|
    /// / avg < tol olmalı. Default 0.15.
    pub channel_parallelism_tol: f64,
    /// P5.4 — Channel minimum süresi (ilk pivot ile son pivot arası
    /// bar sayısı). Default 20.
    pub channel_min_bars: u64,
    /// P5.4 — Channel'in trend olarak sayılabilmesi için minimum |slope|
    /// (fraction per bar). Eşiğin altında rectangle/sideways sayılır.
    /// Default 0.001 (%0.1/bar).
    pub channel_min_slope_pct: f64,
    /// P5.5 — Cup minimum süresi (rim_left → rim_right bar farkı).
    /// Default 30 (Bulkowski: 7 hafta haftalıkta, kısa TF'de oran).
    pub cup_min_bars: u64,
    /// P5.5 — Cup rim eşitliği (sol/sağ rim |price diff|/mid). Default 0.03.
    pub cup_rim_equality_tol: f64,
    /// P5.5 — Cup minimum derinlik oranı (rim'in fraction'ı). Default 0.12.
    pub cup_min_depth_pct: f64,
    /// P5.5 — Cup maksimum derinlik oranı. Default 0.50.
    pub cup_max_depth_pct: f64,
    /// P5.5 — Cup parabolic fit R² eşiği (yuvarlaklık ölçütü).
    /// Default 0.6.
    pub cup_roundness_r2: f64,
    /// P5.5 — Handle derinliğinin cup derinliğine oranı üst sınırı.
    /// Default 0.5.
    pub handle_max_depth_pct_of_cup: f64,
    /// P5.5 — Rounding (saucer) minimum süresi. Cup'tan uzun olur.
    /// Default 40.
    pub rounding_min_bars: u64,
    /// P5.5 — Rounding parabolic fit R² eşiği. Cup'tan biraz daha sıkı.
    /// Default 0.65.
    pub rounding_roundness_r2: f64,

    // ------------------------------------------------------------------
    // Faz 10 Aşama 1 — Triple / Broadening / V / ABCD. Hepsi CLAUDE.md #2
    // uyarınca DB-config kaynaklı, hard-code yok.
    // ------------------------------------------------------------------
    /// Triple top/bottom: 3 tepe/dip arası max göreli sapma.
    pub triple_peak_tol: f64,
    /// Triple top/bottom: pattern ilk→son pivot min bar sayısı.
    pub triple_min_span_bars: u64,
    /// Triple top/bottom: neckline max eğim (fraction per bar).
    pub triple_neckline_slope_max: f64,
    /// Broadening (megaphone) min |slope| (fraction per bar).
    pub broadening_min_slope_pct: f64,
    /// Broadening triangle: "flat" kenarın max |slope|.
    pub broadening_flat_slope_pct: f64,
    /// V-top/V-bottom: baştan sona max bar sayısı.
    pub v_max_total_bars: u64,
    /// V-top/V-bottom: kenarların min göreli genliği.
    pub v_min_amplitude_pct: f64,
    /// V-top/V-bottom: iki kenarın simetri toleransı.
    pub v_symmetry_tol: f64,
    /// ABCD: B→C retracement min oranı (AB'ye göre).
    pub abcd_c_min_retrace: f64,
    /// ABCD: B→C retracement max oranı.
    pub abcd_c_max_retrace: f64,
    /// ABCD: CD bacağının AB'ye göre 1.0'a tolerans.
    pub abcd_d_projection_tol: f64,
    /// ABCD: her bacak min bar sayısı.
    pub abcd_min_bars_per_leg: u64,

    // ------------------------------------------------------------------
    // Faz 10 Aşama 4 — Scallop (J-şekilli bull/bear reversal).
    // ------------------------------------------------------------------
    /// Scallop minimum süresi (rim_left → rim_right bar farkı). Rounding
    /// bottom'dan kısa olabilir. Default 20.
    pub scallop_min_bars: u64,
    /// Scallop rim progress: bull için rim_r - rim_l > rim_l * tol
    /// (breakout ayaklık). Default 0.02 (rim_r rim_l'den en az %2 yukarıda).
    pub scallop_min_rim_progress_pct: f64,
    /// Scallop parabolic fit R² eşiği. Rounding'den biraz daha gevşek
    /// (scallop curve'ü asimetrik). Default 0.55.
    pub scallop_roundness_r2: f64,
}

impl ClassicalConfig {
    pub fn defaults() -> Self {
        Self {
            pivot_level: PivotLevel::L1,
            min_structural_score: 0.50,
            equality_tolerance: 0.03,
            apex_horizon_bars: 50,
            flatness_threshold_pct: 0.001,
            flatness_min_score: 0.3,
            neckline_tolerance_mult: 1.5,
            triangle_symmetry_tol: 0.5,
            hs_max_neckline_slope_pct: 0.003,
            hs_time_symmetry_tol: 0.5,
            rectangle_max_slope_pct: 0.002,
            rectangle_min_bars: 15,
            flag_pole_min_move_atr: 3.0,
            flag_pole_max_bars: 20,
            flag_max_retrace_pct: 0.5,
            flag_atr_period: 14,
            flag_parallelism_tol: 0.3,
            pennant_max_height_pct_of_pole: 0.4,
            channel_parallelism_tol: 0.15,
            channel_min_bars: 20,
            channel_min_slope_pct: 0.001,
            cup_min_bars: 30,
            cup_rim_equality_tol: 0.03,
            cup_min_depth_pct: 0.12,
            cup_max_depth_pct: 0.50,
            cup_roundness_r2: 0.6,
            handle_max_depth_pct_of_cup: 0.5,
            rounding_min_bars: 40,
            rounding_roundness_r2: 0.65,

            triple_peak_tol: 0.03,
            triple_min_span_bars: 10,
            triple_neckline_slope_max: 0.003,
            broadening_min_slope_pct: 0.002,
            broadening_flat_slope_pct: 0.0015,
            v_max_total_bars: 20,
            v_min_amplitude_pct: 0.03,
            v_symmetry_tol: 0.4,
            abcd_c_min_retrace: 0.382,
            abcd_c_max_retrace: 0.886,
            abcd_d_projection_tol: 0.15,
            abcd_min_bars_per_leg: 3,

            scallop_min_bars: 20,
            scallop_min_rim_progress_pct: 0.02,
            scallop_roundness_r2: 0.55,
        }
    }

    pub fn validate(&self) -> ClassicalResult<()> {
        if !(0.0..=1.0).contains(&(self.min_structural_score as f64)) {
            return Err(ClassicalError::InvalidConfig(
                "min_structural_score must be in 0..=1".into(),
            ));
        }
        if !(0.0..=0.25).contains(&self.equality_tolerance) {
            return Err(ClassicalError::InvalidConfig(
                "equality_tolerance must be in 0..=0.25".into(),
            ));
        }
        if self.apex_horizon_bars == 0 {
            return Err(ClassicalError::InvalidConfig(
                "apex_horizon_bars must be > 0".into(),
            ));
        }
        if !(0.0..=0.1).contains(&self.flatness_threshold_pct) {
            return Err(ClassicalError::InvalidConfig(
                "flatness_threshold_pct must be in 0..=0.1".into(),
            ));
        }
        if !(0.0..=1.0).contains(&self.flatness_min_score) {
            return Err(ClassicalError::InvalidConfig(
                "flatness_min_score must be in 0..=1".into(),
            ));
        }
        if !(1.0..=3.0).contains(&self.neckline_tolerance_mult) {
            return Err(ClassicalError::InvalidConfig(
                "neckline_tolerance_mult must be in 1..=3".into(),
            ));
        }
        if !(0.0..=2.0).contains(&self.triangle_symmetry_tol) {
            return Err(ClassicalError::InvalidConfig(
                "triangle_symmetry_tol must be in 0..=2".into(),
            ));
        }
        if !(0.0..=0.05).contains(&self.hs_max_neckline_slope_pct) {
            return Err(ClassicalError::InvalidConfig(
                "hs_max_neckline_slope_pct must be in 0..=0.05".into(),
            ));
        }
        if !(0.0..=2.0).contains(&self.hs_time_symmetry_tol) {
            return Err(ClassicalError::InvalidConfig(
                "hs_time_symmetry_tol must be in 0..=2".into(),
            ));
        }
        if !(0.0..=0.1).contains(&self.rectangle_max_slope_pct) {
            return Err(ClassicalError::InvalidConfig(
                "rectangle_max_slope_pct must be in 0..=0.1".into(),
            ));
        }
        if self.rectangle_min_bars == 0 {
            return Err(ClassicalError::InvalidConfig(
                "rectangle_min_bars must be > 0".into(),
            ));
        }
        if !(0.0..=20.0).contains(&self.flag_pole_min_move_atr) {
            return Err(ClassicalError::InvalidConfig(
                "flag_pole_min_move_atr must be in 0..=20".into(),
            ));
        }
        if self.flag_pole_max_bars == 0 {
            return Err(ClassicalError::InvalidConfig(
                "flag_pole_max_bars must be > 0".into(),
            ));
        }
        if !(0.0..=1.0).contains(&self.flag_max_retrace_pct) {
            return Err(ClassicalError::InvalidConfig(
                "flag_max_retrace_pct must be in 0..=1".into(),
            ));
        }
        if self.flag_atr_period == 0 {
            return Err(ClassicalError::InvalidConfig(
                "flag_atr_period must be > 0".into(),
            ));
        }
        if !(0.0..=2.0).contains(&self.flag_parallelism_tol) {
            return Err(ClassicalError::InvalidConfig(
                "flag_parallelism_tol must be in 0..=2".into(),
            ));
        }
        if !(0.0..=1.0).contains(&self.pennant_max_height_pct_of_pole) {
            return Err(ClassicalError::InvalidConfig(
                "pennant_max_height_pct_of_pole must be in 0..=1".into(),
            ));
        }
        if !(0.0..=2.0).contains(&self.channel_parallelism_tol) {
            return Err(ClassicalError::InvalidConfig(
                "channel_parallelism_tol must be in 0..=2".into(),
            ));
        }
        if self.channel_min_bars == 0 {
            return Err(ClassicalError::InvalidConfig(
                "channel_min_bars must be > 0".into(),
            ));
        }
        if !(0.0..=0.1).contains(&self.channel_min_slope_pct) {
            return Err(ClassicalError::InvalidConfig(
                "channel_min_slope_pct must be in 0..=0.1".into(),
            ));
        }
        if self.cup_min_bars == 0 {
            return Err(ClassicalError::InvalidConfig(
                "cup_min_bars must be > 0".into(),
            ));
        }
        if !(0.0..=0.25).contains(&self.cup_rim_equality_tol) {
            return Err(ClassicalError::InvalidConfig(
                "cup_rim_equality_tol must be in 0..=0.25".into(),
            ));
        }
        if !(0.0..=1.0).contains(&self.cup_min_depth_pct) || !(0.0..=1.0).contains(&self.cup_max_depth_pct) {
            return Err(ClassicalError::InvalidConfig(
                "cup_*_depth_pct must be in 0..=1".into(),
            ));
        }
        if self.cup_min_depth_pct >= self.cup_max_depth_pct {
            return Err(ClassicalError::InvalidConfig(
                "cup_min_depth_pct must be < cup_max_depth_pct".into(),
            ));
        }
        if !(0.0..=1.0).contains(&self.cup_roundness_r2) || !(0.0..=1.0).contains(&self.rounding_roundness_r2) {
            return Err(ClassicalError::InvalidConfig(
                "cup/rounding roundness_r2 must be in 0..=1".into(),
            ));
        }
        if !(0.0..=1.0).contains(&self.handle_max_depth_pct_of_cup) {
            return Err(ClassicalError::InvalidConfig(
                "handle_max_depth_pct_of_cup must be in 0..=1".into(),
            ));
        }
        if self.rounding_min_bars == 0 {
            return Err(ClassicalError::InvalidConfig(
                "rounding_min_bars must be > 0".into(),
            ));
        }
        // Faz 10 Aşama 1 validations.
        if !(0.0..=0.25).contains(&self.triple_peak_tol) {
            return Err(ClassicalError::InvalidConfig(
                "triple_peak_tol must be in 0..=0.25".into(),
            ));
        }
        if self.triple_min_span_bars == 0 {
            return Err(ClassicalError::InvalidConfig(
                "triple_min_span_bars must be > 0".into(),
            ));
        }
        if !(0.0..=0.05).contains(&self.triple_neckline_slope_max) {
            return Err(ClassicalError::InvalidConfig(
                "triple_neckline_slope_max must be in 0..=0.05".into(),
            ));
        }
        if !(0.0..=0.1).contains(&self.broadening_min_slope_pct)
            || !(0.0..=0.1).contains(&self.broadening_flat_slope_pct)
        {
            return Err(ClassicalError::InvalidConfig(
                "broadening slope thresholds must be in 0..=0.1".into(),
            ));
        }
        if self.broadening_flat_slope_pct >= self.broadening_min_slope_pct {
            return Err(ClassicalError::InvalidConfig(
                "broadening_flat_slope_pct must be < broadening_min_slope_pct".into(),
            ));
        }
        if self.v_max_total_bars == 0 {
            return Err(ClassicalError::InvalidConfig(
                "v_max_total_bars must be > 0".into(),
            ));
        }
        if !(0.0..=0.5).contains(&self.v_min_amplitude_pct) {
            return Err(ClassicalError::InvalidConfig(
                "v_min_amplitude_pct must be in 0..=0.5".into(),
            ));
        }
        if !(0.0..=2.0).contains(&self.v_symmetry_tol) {
            return Err(ClassicalError::InvalidConfig(
                "v_symmetry_tol must be in 0..=2".into(),
            ));
        }
        if !(0.0..=1.0).contains(&self.abcd_c_min_retrace)
            || !(0.0..=1.0).contains(&self.abcd_c_max_retrace)
        {
            return Err(ClassicalError::InvalidConfig(
                "abcd_c_* retrace must be in 0..=1".into(),
            ));
        }
        if self.abcd_c_min_retrace >= self.abcd_c_max_retrace {
            return Err(ClassicalError::InvalidConfig(
                "abcd_c_min_retrace must be < abcd_c_max_retrace".into(),
            ));
        }
        if !(0.0..=1.0).contains(&self.abcd_d_projection_tol) {
            return Err(ClassicalError::InvalidConfig(
                "abcd_d_projection_tol must be in 0..=1".into(),
            ));
        }
        if self.abcd_min_bars_per_leg == 0 {
            return Err(ClassicalError::InvalidConfig(
                "abcd_min_bars_per_leg must be > 0".into(),
            ));
        }
        if self.scallop_min_bars == 0 {
            return Err(ClassicalError::InvalidConfig(
                "scallop_min_bars must be > 0".into(),
            ));
        }
        if !(0.0..=0.5).contains(&self.scallop_min_rim_progress_pct) {
            return Err(ClassicalError::InvalidConfig(
                "scallop_min_rim_progress_pct must be in 0..=0.5".into(),
            ));
        }
        if !(0.0..=1.0).contains(&self.scallop_roundness_r2) {
            return Err(ClassicalError::InvalidConfig(
                "scallop_roundness_r2 must be in 0..=1".into(),
            ));
        }
        Ok(())
    }
}
