//! Projection Engine — multi-alternative forward projection with
//! succession rules, Fibonacci price targets, and time estimation.
//!
//! Unlike `projection.rs` which produces a single `Vec<PivotRef>` per
//! detection, this module produces *ranked alternatives* with full
//! leg-by-leg detail suitable for persistence in `wave_projections`.
//!
//! Design: dispatch table per completed formation (CLAUDE.md #1).
//! Each successor rule returns `Vec<ProjectionAlternative>`.


// ─── Output types ───────────────────────────────────────────────────

/// One projected leg within an alternative.
#[derive(Debug, Clone)]
pub struct ProjectedLegSpec {
    pub label: String,
    pub price_start: f64,
    pub price_end: f64,
    pub direction: &'static str,
    pub fib_level: Option<String>,
    /// Estimated duration in bars (relative to previous leg end).
    pub bar_duration: u64,
}

/// One complete alternative scenario.
#[derive(Debug, Clone)]
pub struct ProjectionAlternative {
    pub projected_kind: String,
    pub projected_label: String,
    pub direction: &'static str,
    pub probability: f32,
    pub fib_basis: String,
    pub legs: Vec<ProjectedLegSpec>,
    pub invalidation_price: Option<f64>,
}

/// Input context for projection.
#[derive(Debug, Clone)]
pub struct ProjectionContext {
    /// Subkind of the completed formation (e.g. "impulse_5_bull").
    pub subkind: String,
    /// Realized anchor prices in order [p0, p1, p2, ...].
    pub prices: Vec<f64>,
    /// Average bar spacing of the formation.
    pub avg_bar_spacing: u64,
    /// Wave number within parent (e.g. "3" for wave 3 of impulse).
    /// Used for alternation rule and determining what comes next.
    pub wave_number: Option<String>,
    /// Sibling context: what was wave 2's type? (for alternation).
    pub sibling_w2_kind: Option<String>,
}

// ─── Dispatch table ─────────────────────────────────────────────────

type Successor = fn(&ProjectionContext) -> Vec<ProjectionAlternative>;

const SUCCESSORS: &[(&str, Successor)] = &[
    // Impulse completions — what comes after?
    ("impulse_truncated_5", succeed_impulse_complete),
    ("impulse_w1_extended", succeed_impulse_complete),
    ("impulse_w3_extended", succeed_impulse_complete),
    ("impulse_w5_extended", succeed_impulse_complete),
    ("impulse_5",          succeed_impulse_complete),
    // Corrective completions — trend resumption
    ("zigzag_abc",              succeed_correction_complete),
    ("flat_regular",            succeed_correction_complete),
    ("flat_expanded",           succeed_correction_complete),
    ("flat_running",            succeed_correction_complete),
    ("combination_wxy",         succeed_correction_complete),
    // Triangle — thrust
    ("triangle_contracting",    succeed_triangle),
    ("triangle_expanding",      succeed_triangle),
    ("triangle_barrier",        succeed_triangle),
    ("triangle_running",        succeed_triangle),
    // Diagonals
    ("leading_diagonal_5_3_5",  succeed_leading_diagonal),
    ("ending_diagonal_3_3_3",   succeed_ending_diagonal),
];

/// Main entry point: given a completed formation, produce ranked alternatives.
pub fn project_alternatives(ctx: &ProjectionContext) -> Vec<ProjectionAlternative> {
    let base = ctx.subkind.replace("_bull", "").replace("_bear", "");
    // Use the MAX price across all anchors as reference for clamping
    let ref_price = ctx.prices.iter().cloned().fold(1.0_f64, f64::max);
    for (prefix, func) in SUCCESSORS {
        if base.starts_with(prefix) {
            let mut alts = sanitize_alternatives(func(ctx), ref_price);
            // Sort by probability descending, assign stable order
            alts.sort_by(|a, b| b.probability.partial_cmp(&a.probability).unwrap_or(std::cmp::Ordering::Equal));
            return alts;
        }
    }
    Vec::new()
}

// ─── Helpers ────────────────────────────────────────────────────────

fn dir_str(bullish: bool) -> &'static str {
    if bullish { "bullish" } else { "bearish" }
}

/// Clamp price to sane range: [ref × 0.20, ref × 3.0].
/// Prevents projections going to absurdly low/high values.
fn clamp_price(price: f64, reference: f64) -> f64 {
    let floor = reference * 0.20;
    let ceil = reference * 3.0;
    price.max(floor).min(ceil)
}

/// Sanitize all leg prices in alternatives.
fn sanitize_alternatives(mut alts: Vec<ProjectionAlternative>, ref_price: f64) -> Vec<ProjectionAlternative> {
    for alt in &mut alts {
        for leg in &mut alt.legs {
            leg.price_start = clamp_price(leg.price_start, ref_price);
            leg.price_end = clamp_price(leg.price_end, ref_price);
        }
        if let Some(inv) = &mut alt.invalidation_price {
            *inv = clamp_price(*inv, ref_price);
        }
    }
    alts
}

/// Build an ABC correction alternative.
fn make_abc(
    last_price: f64,
    impulse_range: f64,
    bull: bool,          // direction of the CORRECTION (opposite of impulse)
    kind: &str,
    label: &str,
    probability: f32,
    a_fib: f64,
    b_retrace: f64,
    c_fib: f64,
    bar_spacing: u64,
    inv_price: Option<f64>,
) -> ProjectionAlternative {
    let sign = if bull { 1.0 } else { -1.0 };
    let a_end = last_price + sign * a_fib * impulse_range;
    let a_retrace = (a_end - last_price).abs();
    let b_end = a_end - sign * b_retrace * a_retrace;
    let c_end = last_price + sign * c_fib * impulse_range;

    ProjectionAlternative {
        projected_kind: kind.to_string(),
        projected_label: label.to_string(),
        direction: dir_str(bull),
        probability,
        fib_basis: format!("A={a_fib} B={b_retrace} C={c_fib}"),
        legs: vec![
            ProjectedLegSpec {
                label: "A".into(),
                price_start: last_price,
                price_end: a_end,
                direction: dir_str(bull),
                fib_level: Some(format!("{a_fib} retrace")),
                bar_duration: bar_spacing,
            },
            ProjectedLegSpec {
                label: "B".into(),
                price_start: a_end,
                price_end: b_end,
                direction: dir_str(!bull),
                fib_level: Some(format!("{b_retrace} of A")),
                bar_duration: (bar_spacing as f64 * 0.618) as u64,
            },
            ProjectedLegSpec {
                label: "C".into(),
                price_start: b_end,
                price_end: c_end,
                direction: dir_str(bull),
                fib_level: Some(format!("{c_fib} retrace")),
                bar_duration: bar_spacing,
            },
        ],
        invalidation_price: inv_price,
    }
}

// ─── Successors ─────────────────────────────────────────────────────

/// After impulse 1-2-3-4-5 completes: project corrective alternatives.
fn succeed_impulse_complete(ctx: &ProjectionContext) -> Vec<ProjectionAlternative> {
    if ctx.prices.len() < 6 { return Vec::new(); }
    let p0 = ctx.prices[0];
    let p5 = ctx.prices[5];
    let range = (p5 - p0).abs();
    if range == 0.0 { return Vec::new(); }

    let imp_bull = p5 > p0;
    let corr_bull = !imp_bull; // correction goes opposite
    let step = ctx.avg_bar_spacing;

    // Alternation: if wave 2 was zigzag, prefer flat/triangle for the correction
    let w2_was_zigzag = ctx.sibling_w2_kind
        .as_ref()
        .map(|k| k.contains("zigzag"))
        .unwrap_or(false);

    let (prob_zz, prob_flat, prob_tri, prob_wxy) = if w2_was_zigzag {
        (0.15, 0.40, 0.25, 0.20) // Alternation: prefer flat/triangle
    } else {
        (0.40, 0.25, 0.15, 0.20) // Default: zigzag most common
    };

    // Wave 1 end = p1 — correction can't go beyond this for most types
    let p1 = ctx.prices[1];

    let mut alts = Vec::new();

    // Alt 1: Zigzag A-B-C (sharp correction)
    alts.push(make_abc(
        p5, range, corr_bull,
        "zigzag_abc", "Zigzag (A-B-C)",
        prob_zz,
        0.382, 0.50, 0.618,
        step,
        Some(p0), // full retrace = invalidation
    ));

    // Alt 2: Flat Regular (sideways correction)
    alts.push(make_abc(
        p5, range, corr_bull,
        "flat_regular", "Flat (Regular)",
        prob_flat,
        0.236, 0.786, 0.382,
        step,
        Some(p1), // wave 1 territory = warning zone
    ));

    // Alt 3: Triangle (consolidation)
    {
        let sign = if corr_bull { 1.0 } else { -1.0 };
        let a_end = p5 + sign * 0.382 * range;
        let b_end = a_end - sign * 0.786 * (a_end - p5).abs();
        let c_end = b_end + sign * 0.618 * (a_end - p5).abs();
        let d_end = c_end - sign * 0.618 * (c_end - b_end).abs();
        let e_end = d_end + sign * 0.50 * (c_end - b_end).abs();

        alts.push(ProjectionAlternative {
            projected_kind: "triangle_contracting".into(),
            projected_label: "Triangle (Contracting)".into(),
            direction: dir_str(corr_bull),
            probability: prob_tri,
            fib_basis: "contracting ABCDE".into(),
            legs: vec![
                ProjectedLegSpec { label: "A".into(), price_start: p5, price_end: a_end, direction: dir_str(corr_bull), fib_level: Some("0.382 retrace".into()), bar_duration: step },
                ProjectedLegSpec { label: "B".into(), price_start: a_end, price_end: b_end, direction: dir_str(!corr_bull), fib_level: Some("0.786 of A".into()), bar_duration: step },
                ProjectedLegSpec { label: "C".into(), price_start: b_end, price_end: c_end, direction: dir_str(corr_bull), fib_level: Some("0.618 of A".into()), bar_duration: step },
                ProjectedLegSpec { label: "D".into(), price_start: c_end, price_end: d_end, direction: dir_str(!corr_bull), fib_level: Some("0.618 of BC".into()), bar_duration: step },
                ProjectedLegSpec { label: "E".into(), price_start: d_end, price_end: e_end, direction: dir_str(corr_bull), fib_level: Some("0.50 of BC".into()), bar_duration: (step as f64 * 0.618) as u64 },
            ],
            invalidation_price: Some(p0),
        });
    }

    // Alt 4: WXY Combination
    {
        let sign = if corr_bull { 1.0 } else { -1.0 };
        let w_end = p5 + sign * 0.382 * range;
        let x_end = w_end - sign * 0.618 * (w_end - p5).abs();
        let y_end = p5 + sign * 0.500 * range;

        alts.push(ProjectionAlternative {
            projected_kind: "combination_wxy".into(),
            projected_label: "WXY Combination".into(),
            direction: dir_str(corr_bull),
            probability: prob_wxy,
            fib_basis: "W=0.382 X=0.618 Y=0.500".into(),
            legs: vec![
                ProjectedLegSpec { label: "W".into(), price_start: p5, price_end: w_end, direction: dir_str(corr_bull), fib_level: Some("0.382 retrace".into()), bar_duration: step * 2 },
                ProjectedLegSpec { label: "X".into(), price_start: w_end, price_end: x_end, direction: dir_str(!corr_bull), fib_level: Some("0.618 of W".into()), bar_duration: step },
                ProjectedLegSpec { label: "Y".into(), price_start: x_end, price_end: y_end, direction: dir_str(corr_bull), fib_level: Some("0.500 retrace".into()), bar_duration: step * 2 },
            ],
            invalidation_price: Some(p0),
        });
    }

    alts
}

/// After a correction completes: project trend resumption (impulse).
fn succeed_correction_complete(ctx: &ProjectionContext) -> Vec<ProjectionAlternative> {
    if ctx.prices.len() < 2 { return Vec::new(); }
    let p_start = ctx.prices[0];
    let p_end = *ctx.prices.last().unwrap();
    let corr_range = (p_end - p_start).abs();
    if corr_range == 0.0 { return Vec::new(); }

    let corr_bull = p_end > p_start;
    let trend_bull = !corr_bull; // trend resumes opposite to correction
    let step = ctx.avg_bar_spacing;

    let mut alts = Vec::new();

    // Alt 1: Strong impulse continuation (1.618 extension)
    {
        let sign = if trend_bull { 1.0 } else { -1.0 };
        let target_1 = p_end + sign * 1.0 * corr_range;
        let target_2 = p_end + sign * 1.618 * corr_range;

        alts.push(ProjectionAlternative {
            projected_kind: "impulse_5".into(),
            projected_label: "Impulse (Trend Resumption)".into(),
            direction: dir_str(trend_bull),
            probability: 0.55,
            fib_basis: "1.0–1.618× correction range".into(),
            legs: vec![
                ProjectedLegSpec { label: "1".into(), price_start: p_end, price_end: target_1, direction: dir_str(trend_bull), fib_level: Some("1.0× correction".into()), bar_duration: step * 2 },
                ProjectedLegSpec { label: "2".into(), price_start: target_1, price_end: p_end + sign * 0.618 * corr_range, direction: dir_str(!trend_bull), fib_level: Some("0.382 retrace of 1".into()), bar_duration: step },
                ProjectedLegSpec { label: "3".into(), price_start: p_end + sign * 0.618 * corr_range, price_end: target_2, direction: dir_str(trend_bull), fib_level: Some("1.618× correction".into()), bar_duration: step * 3 },
            ],
            invalidation_price: Some(p_end - sign * 0.1 * corr_range),
        });
    }

    // Alt 2: Deeper correction (X-Y extension of WXY)
    {
        let sign = if corr_bull { 1.0 } else { -1.0 }; // continues correction direction
        let y_end = p_end + sign * 0.618 * corr_range;

        alts.push(ProjectionAlternative {
            projected_kind: "combination_wxy".into(),
            projected_label: "Extended Correction (X-Y)".into(),
            direction: dir_str(corr_bull),
            probability: 0.25,
            fib_basis: "continuation 0.618× extension".into(),
            legs: vec![
                ProjectedLegSpec { label: "X".into(), price_start: p_end, price_end: p_end - sign * 0.500 * corr_range, direction: dir_str(!corr_bull), fib_level: Some("0.500 retrace".into()), bar_duration: step },
                ProjectedLegSpec { label: "Y".into(), price_start: p_end - sign * 0.500 * corr_range, price_end: y_end, direction: dir_str(corr_bull), fib_level: Some("0.618 extension".into()), bar_duration: step * 2 },
            ],
            invalidation_price: None,
        });
    }

    // Alt 3: Shallow continuation (weaker trend)
    {
        let sign = if trend_bull { 1.0 } else { -1.0 };
        let target = p_end + sign * 0.618 * corr_range;

        alts.push(ProjectionAlternative {
            projected_kind: "impulse_5".into(),
            projected_label: "Weak Impulse".into(),
            direction: dir_str(trend_bull),
            probability: 0.20,
            fib_basis: "0.618× correction range".into(),
            legs: vec![
                ProjectedLegSpec { label: "1".into(), price_start: p_end, price_end: target, direction: dir_str(trend_bull), fib_level: Some("0.618× correction".into()), bar_duration: step * 2 },
            ],
            invalidation_price: Some(p_end),
        });
    }

    alts
}

/// After triangle: thrust projection.
fn succeed_triangle(ctx: &ProjectionContext) -> Vec<ProjectionAlternative> {
    if ctx.prices.len() < 6 { return Vec::new(); }
    let max_p = ctx.prices.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let min_p = ctx.prices.iter().cloned().fold(f64::INFINITY, f64::min);
    let width = max_p - min_p;
    if width == 0.0 { return Vec::new(); }

    let e_price = *ctx.prices.last().unwrap();
    let d_price = ctx.prices[ctx.prices.len() - 2];
    let last_leg_bull = e_price > d_price;
    let thrust_bull = !last_leg_bull; // thrust opposes final leg
    let sign = if thrust_bull { 1.0 } else { -1.0 };
    let step = ctx.avg_bar_spacing;

    vec![
        ProjectionAlternative {
            projected_kind: "impulse_5".into(),
            projected_label: "Triangle Thrust".into(),
            direction: dir_str(thrust_bull),
            probability: 0.75,
            fib_basis: format!("width={width:.0}"),
            legs: vec![
                ProjectedLegSpec { label: "thrust".into(), price_start: e_price, price_end: e_price + sign * width, direction: dir_str(thrust_bull), fib_level: Some("1.0× triangle width".into()), bar_duration: step * 2 },
            ],
            invalidation_price: Some(e_price - sign * width * 0.5),
        },
        ProjectionAlternative {
            projected_kind: "impulse_5".into(),
            projected_label: "Extended Thrust".into(),
            direction: dir_str(thrust_bull),
            probability: 0.25,
            fib_basis: format!("1.618× width={width:.0}"),
            legs: vec![
                ProjectedLegSpec { label: "thrust".into(), price_start: e_price, price_end: e_price + sign * 1.618 * width, direction: dir_str(thrust_bull), fib_level: Some("1.618× triangle width".into()), bar_duration: step * 3 },
            ],
            invalidation_price: Some(e_price - sign * width * 0.5),
        },
    ]
}

/// Leading diagonal → strong continuation.
fn succeed_leading_diagonal(ctx: &ProjectionContext) -> Vec<ProjectionAlternative> {
    if ctx.prices.len() < 6 { return Vec::new(); }
    let p0 = ctx.prices[0];
    let p5 = *ctx.prices.last().unwrap();
    let range = (p5 - p0).abs();
    if range == 0.0 { return Vec::new(); }

    let bull = p5 > p0;
    let sign = if bull { 1.0 } else { -1.0 };
    let step = ctx.avg_bar_spacing;

    vec![
        ProjectionAlternative {
            projected_kind: "impulse_5".into(),
            projected_label: "Impulse Continuation".into(),
            direction: dir_str(bull),
            probability: 0.65,
            fib_basis: "1.0× diagonal range".into(),
            legs: vec![
                ProjectedLegSpec { label: "3".into(), price_start: p5, price_end: p5 + sign * range, direction: dir_str(bull), fib_level: Some("1.0× diagonal".into()), bar_duration: step * 3 },
            ],
            invalidation_price: Some(p0),
        },
        ProjectionAlternative {
            projected_kind: "impulse_5".into(),
            projected_label: "Strong Continuation".into(),
            direction: dir_str(bull),
            probability: 0.35,
            fib_basis: "1.618× diagonal range".into(),
            legs: vec![
                ProjectedLegSpec { label: "3".into(), price_start: p5, price_end: p5 + sign * 1.618 * range, direction: dir_str(bull), fib_level: Some("1.618× diagonal".into()), bar_duration: step * 4 },
            ],
            invalidation_price: Some(p0),
        },
    ]
}

/// Ending diagonal → sharp reversal.
fn succeed_ending_diagonal(ctx: &ProjectionContext) -> Vec<ProjectionAlternative> {
    if ctx.prices.len() < 6 { return Vec::new(); }
    let p0 = ctx.prices[0];
    let p5 = *ctx.prices.last().unwrap();
    let range = (p5 - p0).abs();
    if range == 0.0 { return Vec::new(); }

    let bull = p5 > p0;
    let rev_bull = !bull; // reversal opposes diagonal
    let sign = if rev_bull { 1.0 } else { -1.0 };
    let step = ctx.avg_bar_spacing;

    vec![
        ProjectionAlternative {
            projected_kind: "zigzag_abc".into(),
            projected_label: "Sharp Reversal (Full Retrace)".into(),
            direction: dir_str(rev_bull),
            probability: 0.60,
            fib_basis: "full retrace to diagonal start".into(),
            legs: vec![
                ProjectedLegSpec { label: "A".into(), price_start: p5, price_end: p5 + sign * 0.618 * range, direction: dir_str(rev_bull), fib_level: Some("0.618 retrace".into()), bar_duration: step },
                ProjectedLegSpec { label: "B".into(), price_start: p5 + sign * 0.618 * range, price_end: p5 + sign * 0.382 * range, direction: dir_str(!rev_bull), fib_level: Some("0.382 of A".into()), bar_duration: (step as f64 * 0.618) as u64 },
                ProjectedLegSpec { label: "C".into(), price_start: p5 + sign * 0.382 * range, price_end: p0, direction: dir_str(rev_bull), fib_level: Some("full retrace".into()), bar_duration: step * 2 },
            ],
            invalidation_price: None,
        },
        ProjectionAlternative {
            projected_kind: "zigzag_abc".into(),
            projected_label: "Partial Reversal".into(),
            direction: dir_str(rev_bull),
            probability: 0.40,
            fib_basis: "0.618 retrace".into(),
            legs: vec![
                ProjectedLegSpec { label: "rev".into(), price_start: p5, price_end: p5 + sign * 0.618 * range, direction: dir_str(rev_bull), fib_level: Some("0.618 retrace".into()), bar_duration: step * 2 },
            ],
            invalidation_price: None,
        },
    ]
}
