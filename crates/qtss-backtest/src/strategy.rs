use qtss_domain::bar::TimestampBar;

use crate::engine::BacktestContext;

/// Strateji: her barda çağrılır. IO gerektiren stratejiler için ayrı `async` varyant ileride eklenebilir.
pub trait Strategy: Send {
    fn name(&self) -> &'static str;

    fn on_bar(&mut self, ctx: &mut BacktestContext, bar: &TimestampBar);
}
