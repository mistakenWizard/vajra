//! Fill models: decide which bar and which price a decision executes at.
//! The default (`NextBarOpen`) is lookahead-free; `SameBarClose` reproduces
//! pre-SP1 (v0.1) behavior exactly and is used to pin golden tests.

use crate::marketdata::Quote;

/// Which OHLC field of the fill bar to price an underlying/EQ leg from.
#[derive(Debug, Clone, Copy)]
pub enum BarField {
    Open,
    High,
    Low,
    Close,
}

/// Resolves the bar and price a decision fills at.
pub trait FillModel: Send {
    /// Bar index a decision made on `decision_i` fills at.
    fn fill_bar(&self, decision_i: usize) -> usize;
    /// Underlying OHLC field to synthesize/mark from at the fill bar.
    fn underlying_field(&self) -> BarField;
    /// Price to take from a real option `Quote` at the fill bar.
    fn quote_price(&self, q: &Quote) -> f64;
}

/// Lookahead-free default: a decision on bar `t` fills at `t+1`'s open.
pub struct NextBarOpen;
impl FillModel for NextBarOpen {
    fn fill_bar(&self, decision_i: usize) -> usize {
        decision_i + 1
    }
    fn underlying_field(&self) -> BarField {
        BarField::Open
    }
    fn quote_price(&self, q: &Quote) -> f64 {
        q.open
    }
}

/// Pre-SP1 behavior: fills on the decision bar at its close (has lookahead).
/// Opt-in only, so historical goldens can pin exact numbers.
pub struct SameBarClose;
impl FillModel for SameBarClose {
    fn fill_bar(&self, decision_i: usize) -> usize {
        decision_i
    }
    fn underlying_field(&self) -> BarField {
        BarField::Close
    }
    fn quote_price(&self, q: &Quote) -> f64 {
        q.close
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::marketdata::Quote;

    fn q() -> Quote {
        Quote {
            open: 1.0,
            high: 2.0,
            low: 0.5,
            close: 1.5,
            bid: None,
            ask: None,
        }
    }

    #[test]
    fn next_bar_open_fills_at_next_bar_open() {
        let m = NextBarOpen;
        assert_eq!(m.fill_bar(3), 4);
        assert!(matches!(m.underlying_field(), BarField::Open));
        assert_eq!(m.quote_price(&q()), 1.0);
    }

    #[test]
    fn same_bar_close_fills_at_this_bar_close() {
        let m = SameBarClose;
        assert_eq!(m.fill_bar(3), 3);
        assert!(matches!(m.underlying_field(), BarField::Close));
        assert_eq!(m.quote_price(&q()), 1.5);
    }
}
