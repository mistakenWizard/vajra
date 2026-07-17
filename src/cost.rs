use crate::engine::Trade;

pub trait CostModel: Send {
    /// Adjust theoretical fill price for slippage.
    /// `entry_action` is the leg's original action ("BUY"/"SELL"); `is_exit`
    /// flips the direction for a closing fill. `is_option` selects the
    /// option (flat points) vs equity (proportional) rule.
    fn adjust_fill(&self, mid: f64, entry_action: &str, is_option: bool, is_exit: bool) -> f64;
    /// Spread-aware variant. `spread` is the quoted `(ask - bid)` when known.
    /// Default ignores it and applies the flat `adjust_fill` rule.
    fn adjust_fill_spread(
        &self,
        mid: f64,
        entry_action: &str,
        is_option: bool,
        is_exit: bool,
        spread: Option<f64>,
    ) -> f64 {
        let _ = spread;
        self.adjust_fill(mid, entry_action, is_option, is_exit)
    }
    /// Total friction charged once on a completed round-trip trade.
    fn round_trip_cost(&self, trade: &Trade, lot_size: f64) -> f64;
}

pub struct ZeroCost;
impl CostModel for ZeroCost {
    fn adjust_fill(&self, mid: f64, _entry_action: &str, _is_option: bool, _is_exit: bool) -> f64 {
        mid
    }
    fn round_trip_cost(&self, _trade: &Trade, _lot_size: f64) -> f64 {
        0.0
    }
}

pub struct IndiaOptionsCost;
impl CostModel for IndiaOptionsCost {
    fn adjust_fill(&self, mid: f64, entry_action: &str, is_option: bool, is_exit: bool) -> f64 {
        // Direction: on entry, SELL reduces / BUY adds. On exit it flips.
        let adds = if is_exit {
            entry_action == "SELL"
        } else {
            entry_action == "BUY"
        };
        if is_option {
            if adds {
                mid + 0.5
            } else {
                mid - 0.5
            }
        } else if adds {
            mid + mid * 0.0005
        } else {
            mid - mid * 0.0005
        }
    }

    fn adjust_fill_spread(
        &self,
        mid: f64,
        entry_action: &str,
        is_option: bool,
        is_exit: bool,
        spread: Option<f64>,
    ) -> f64 {
        match (is_option, spread) {
            (true, Some(s)) => {
                // Cross a quarter of the quoted spread adversely (vs the flat 0.5 points).
                // ponytail: 0.25 is a fixed fraction; make it configurable if a venue needs it.
                let adds = if is_exit {
                    entry_action == "SELL"
                } else {
                    entry_action == "BUY"
                };
                if adds {
                    mid + s * 0.25
                } else {
                    mid - s * 0.25
                }
            }
            _ => self.adjust_fill(mid, entry_action, is_option, is_exit),
        }
    }

    fn round_trip_cost(&self, trade: &Trade, lot_size: f64) -> f64 {
        let base_brokerage = 40.0 * (trade.legs.len() as f64 / 2.0); // 40 Rs per roundtrip spread
        let gst = base_brokerage * 0.18; // 18% GST on brokerage
        let total_brokerage = base_brokerage + gst;
        let stt = if trade.entry_price > 0.0 {
            trade.entry_price * trade.quantity as f64 * lot_size * 0.00125
        } else {
            0.0
        };
        total_brokerage + stt
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spread_slippage_widens_monotonically_on_buy() {
        let c = IndiaOptionsCost;
        // BUY entry pays up: wider spread => higher fill.
        let f0 = c.adjust_fill_spread(10.0, "BUY", true, false, Some(0.0));
        let f1 = c.adjust_fill_spread(10.0, "BUY", true, false, Some(2.0));
        let f2 = c.adjust_fill_spread(10.0, "BUY", true, false, Some(4.0));
        assert!(f1 > f0 && f2 > f1, "fills {f0} {f1} {f2}");
    }

    #[test]
    fn no_spread_matches_flat_adjust_fill() {
        let c = IndiaOptionsCost;
        let flat = c.adjust_fill(10.0, "BUY", true, false);
        let none = c.adjust_fill_spread(10.0, "BUY", true, false, None);
        assert!((flat - none).abs() < 1e-12);
    }

    #[test]
    fn default_method_ignores_spread() {
        let c = ZeroCost;
        assert_eq!(
            c.adjust_fill_spread(10.0, "BUY", true, false, Some(4.0)),
            10.0
        );
    }
}
