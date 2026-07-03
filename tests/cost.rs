use vajra::cost::{CostModel, IndiaOptionsCost, ZeroCost};
use vajra::engine::{OptionLeg, Trade};

fn trade_with_legs(n_legs: usize, entry_price: f64) -> Trade {
    let legs = (0..n_legs)
        .map(|_| OptionLeg {
            strike: 20000.0,
            option_type: "CE".into(),
            entry_price: 100.0,
            exit_price: None,
            action: "SELL".into(),
            expiry_days: Some(7.0),
        })
        .collect();
    Trade {
        symbol: "X".into(),
        entry_time: 0,
        exit_time: None,
        entry_price,
        exit_price: None,
        quantity: 1,
        pnl: None,
        direction: "SPREAD".into(),
        legs,
    }
}

#[test]
fn zero_cost_is_frictionless() {
    let c = ZeroCost;
    assert_eq!(c.round_trip_cost(&trade_with_legs(2, 100.0), 50.0), 0.0);
    assert_eq!(c.adjust_fill(100.0, "SELL", true, false), 100.0);
}

#[test]
fn india_options_cost_slippage() {
    let c = IndiaOptionsCost;
    // entry SELL: -0.5
    assert_eq!(c.adjust_fill(100.0, "SELL", true, false), 99.5);
    // entry BUY: +0.5
    assert_eq!(c.adjust_fill(100.0, "BUY", true, false), 100.5);
    // exit SELL (buy-to-close): +0.5
    assert_eq!(c.adjust_fill(100.0, "SELL", true, true), 100.5);
}

#[test]
fn india_options_cost_round_trip() {
    let c = IndiaOptionsCost;
    // 2 legs, entry_price 100, lot_size 50, quantity 1:
    // base_brokerage = 40 * (2/2) = 40; gst = 7.2; total = 47.2
    // stt = 100 * 1 * 50 * 0.00125 = 6.25; total = 53.45
    let got = c.round_trip_cost(&trade_with_legs(2, 100.0), 50.0);
    assert!((got - 53.45).abs() < 1e-9, "got {got}");
}
