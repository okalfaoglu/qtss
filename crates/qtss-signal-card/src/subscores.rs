use qtss_chart_patterns::SignalDashboardV1;

/// Turkish strength word aligned with `docs/SIGNAL_MACHINE_GROUP_POLICY.md`.
#[must_use]
pub fn strength_label_tr(score: u8) -> &'static str {
    let s = score.min(10);
    if s >= 7 {
        "GÜÇLÜ"
    } else if s == 6 {
        "ORTA"
    } else if s >= 4 {
        "ZAYIF"
    } else {
        "KRİTİK"
    }
}

/// Returns `(t, t_max, m, m_max, r, r_max)` for the `T:4/4 · M:1/3 · R:3/3` line.
#[must_use]
pub fn subscores_tmr(dash: &SignalDashboardV1, durum_upper: &str) -> (u8, u8, u8, u8, u8, u8) {
    let long = durum_upper == "LONG";
    let short = durum_upper == "SHORT";

    let mut t = 0_u8;
    let local_ok = (long && dash.yerel_trend == "YUKARI")
        || (short && dash.yerel_trend == "ASAGI")
        || dash.yerel_trend == "YATAY";
    if local_ok {
        t += 2;
    }
    if dash.global_trend != "KAPALI" {
        t += 1;
    }
    if dash.piyasa_modu != "BELIRSIZ" {
        t += 1;
    }
    if long || short {
        t += 1;
    }
    t = t.min(4);

    let mut m = 0_u8;
    if long {
        if dash.momentum_1 == "POZITIF" {
            m += 1;
        }
        if dash.momentum_2 == "POZITIF" {
            m += 1;
        }
    } else if short {
        if dash.momentum_1 == "NEGATIF" {
            m += 1;
        }
        if dash.momentum_2 == "NEGATIF" {
            m += 1;
        }
    }
    if !dash.trend_tukenmesi {
        m += 1;
    }
    m = m.min(3);

    let mut r = 0_u8;
    if let (Some(e), Some(sl), Some(tp)) = (dash.giris_gercek, dash.stop_ilk, dash.kar_al_ilk) {
        if e.is_finite() && sl.is_finite() && tp.is_finite() && e.abs() > 1e-12 {
            let (risk, reward) = if long {
                ((e - sl).abs(), (tp - e).abs())
            } else if short {
                ((sl - e).abs(), (e - tp).abs())
            } else {
                (0.0, 0.0)
            };
            if risk > 1e-9 {
                let rr = reward / risk;
                r = if rr >= 2.5 {
                    3
                } else if rr >= 1.5 {
                    2
                } else if rr >= 0.8 {
                    1
                } else {
                    0
                };
            }
        }
    }

    (t, 4, m, 3, r, 3)
}
