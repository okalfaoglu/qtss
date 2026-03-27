use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

use crate::engine::{ClosedTrade, EquityPoint};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceReport {
    pub total_return: Decimal,
    pub cagr: Decimal,
    pub sharpe: Decimal,
    pub sortino: Decimal,
    pub max_drawdown: Decimal,
    pub calmar: Decimal,
    pub win_rate: Decimal,
    pub profit_factor: Decimal,
    pub max_consecutive_losses: i32,
}

impl PerformanceReport {
    pub fn from_equity_and_trades(
        curve: &[EquityPoint],
        trades: &[ClosedTrade],
        initial: Decimal,
    ) -> Self {
        let last = curve.last().map(|p| p.equity).unwrap_or(initial);
        let total_return = if initial.is_zero() {
            Decimal::ZERO
        } else {
            (last - initial) / initial
        };

        let max_dd = max_drawdown(curve);
        let sharpe = sharpe_ratio_simple(curve, dec!(0)); // rf = 0 MVP
        let sortino = sortino_ratio_simple(curve, dec!(0));

        let (win_rate, profit_factor, max_cons_loss) = trade_stats(trades);

        Self {
            total_return,
            cagr: Decimal::ZERO, // tarih aralığından hesaplanacak — MVP placeholder
            sharpe,
            sortino,
            max_drawdown: max_dd,
            calmar: if max_dd.is_zero() {
                Decimal::ZERO
            } else {
                total_return / max_dd.abs()
            },
            win_rate,
            profit_factor,
            max_consecutive_losses: max_cons_loss,
        }
    }
}

fn max_drawdown(curve: &[EquityPoint]) -> Decimal {
    let mut peak = Decimal::MIN;
    let mut max_dd = Decimal::ZERO;
    for p in curve {
        if p.equity > peak {
            peak = p.equity;
        }
        if peak > Decimal::ZERO {
            let dd = (p.equity - peak) / peak;
            if dd < max_dd {
                max_dd = dd;
            }
        }
    }
    max_dd
}

fn period_returns(curve: &[EquityPoint]) -> Vec<Decimal> {
    curve
        .windows(2)
        .filter_map(|w| {
            let a = w[0].equity;
            let b = w[1].equity;
            if a.is_zero() {
                None
            } else {
                Some((b - a) / a)
            }
        })
        .collect()
}

fn mean(xs: &[Decimal]) -> Decimal {
    if xs.is_empty() {
        return Decimal::ZERO;
    }
    xs.iter().copied().sum::<Decimal>() / Decimal::from(xs.len() as u64)
}

fn variance(xs: &[Decimal], m: Decimal) -> Decimal {
    if xs.is_empty() {
        return Decimal::ZERO;
    }
    let mut s = Decimal::ZERO;
    for x in xs {
        let d = *x - m;
        s += d * d;
    }
    s / Decimal::from(xs.len().saturating_sub(1).max(1) as u64)
}

fn sharpe_ratio_simple(curve: &[EquityPoint], rf: Decimal) -> Decimal {
    let rets = period_returns(curve);
    if rets.is_empty() {
        return Decimal::ZERO;
    }
    let m = mean(&rets);
    let v = variance(&rets, m);
    let sd = v.sqrt().unwrap_or(Decimal::ZERO);
    if sd.is_zero() {
        return Decimal::ZERO;
    }
    (m - rf) / sd
}

fn sortino_ratio_simple(curve: &[EquityPoint], rf: Decimal) -> Decimal {
    let rets = period_returns(curve);
    if rets.is_empty() {
        return Decimal::ZERO;
    }
    let m = mean(&rets);
    let downs: Vec<Decimal> = rets
        .iter()
        .filter(|r| **r < rf)
        .map(|r| (r - rf) * (r - rf))
        .collect();
    if downs.is_empty() {
        return Decimal::ZERO;
    }
    let down_dev = (mean(&downs)).sqrt().unwrap_or(Decimal::ZERO);
    if down_dev.is_zero() {
        return Decimal::ZERO;
    }
    (m - rf) / down_dev
}

fn trade_stats(trades: &[ClosedTrade]) -> (Decimal, Decimal, i32) {
    if trades.is_empty() {
        return (Decimal::ZERO, Decimal::ZERO, 0);
    }
    let mut wins = 0usize;
    let mut gross_profit = Decimal::ZERO;
    let mut gross_loss = Decimal::ZERO;
    let mut max_cons = 0i32;
    let mut cur_cons = 0i32;
    for t in trades {
        if t.pnl >= Decimal::ZERO {
            wins += 1;
            gross_profit += t.pnl;
            cur_cons = 0;
        } else {
            gross_loss += t.pnl.abs();
            cur_cons += 1;
            if cur_cons > max_cons {
                max_cons = cur_cons;
            }
        }
    }
    let n = trades.len() as u64;
    let win_rate = Decimal::from(wins as u64) / Decimal::from(n);
    let profit_factor = if gross_loss.is_zero() {
        Decimal::MAX
    } else {
        gross_profit / gross_loss
    };
    (win_rate, profit_factor, max_cons)
}

// Decimal sqrt — rust_decimal has sqrt in newer versions
trait DecimalExt {
    fn sqrt(self) -> Option<Decimal>;
}

impl DecimalExt for Decimal {
    fn sqrt(self) -> Option<Decimal> {
        if self < Decimal::ZERO {
            return None;
        }
        if self.is_zero() {
            return Some(Decimal::ZERO);
        }
        // Newton: x_{n+1} = (x + S/x)/2
        let mut x = self;
        let s = self;
        for _ in 0..32 {
            let nx = (x + s / x) / dec!(2);
            if (nx - x).abs() < dec!(0.0000000001) {
                return Some(nx);
            }
            x = nx;
        }
        Some(x)
    }
}
