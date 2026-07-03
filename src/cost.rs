use crate::engine::Trade;

pub trait CostModel: Send {
    /// Adjust theoretical fill price for slippage.
    /// `entry_action` is the leg's original action ("BUY"/"SELL"); `is_exit`
    /// flips the direction for a closing fill. `is_option` selects the
    /// option (flat points) vs equity (proportional) rule.
    fn adjust_fill(&self, mid: f64, entry_action: &str, is_option: bool, is_exit: bool) -> f64;
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
