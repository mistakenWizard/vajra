use polars::prelude::*;
use std::collections::HashMap;
use vajra::engine::{BacktestEngine, OptionLeg, Signal, SignalAction, Strategy};
use vajra::strategies::MovingAverageCrossover;

fn load(path: &str) -> DataFrame {
    CsvReader::from_path(path)
        .unwrap()
        .has_header(true)
        .finish()
        .unwrap()
}

#[test]
fn golden_equity_ma_crossover() {
    let df = load("tests/fixtures/eq_ohlcv.csv");
    let mut strat = MovingAverageCrossover::new();
    strat.init(&HashMap::new());
    let mut engine =
        BacktestEngine::new(100_000.0).with_fill_model(Box::new(vajra::fill::SameBarClose));
    let trades = engine.run(&df, &mut strat).unwrap();

    // CAPTURE-THEN-FREEZE: run once, read the actual values from the failure,
    // paste them here, and re-run to green. Freeze count + last trade pnl.
    assert_eq!(trades.len(), 1, "trade count");
    let pnl = trades[0].pnl.unwrap();
    assert!((pnl - 15531.140700000002).abs() < 1e-6, "pnl was {pnl}"); // captured, frozen
}

struct FixedStraddle {
    bar: usize,
}
impl Strategy for FixedStraddle {
    fn init(&mut self, _p: &HashMap<String, serde_json::Value>) {}
    fn on_bar(
        &mut self,
        _ts: i64,
        _o: f64,
        _h: f64,
        _l: f64,
        c: f64,
        _v: f64,
        _pnl: Option<f64>,
    ) -> Option<Signal> {
        self.bar += 1;
        let atm = (c / 100.0).round() * 100.0;
        if self.bar == 5 {
            let legs = vec![
                OptionLeg {
                    strike: atm,
                    option_type: "CE".into(),
                    entry_price: 0.0,
                    exit_price: None,
                    action: "SELL".into(),
                    expiry_days: Some(7.0),
                },
                OptionLeg {
                    strike: atm,
                    option_type: "PE".into(),
                    entry_price: 0.0,
                    exit_price: None,
                    action: "SELL".into(),
                    expiry_days: Some(7.0),
                },
            ];
            return Some(Signal {
                action: SignalAction::OpenPosition {
                    legs,
                    expiry_days: Some(7.0),
                },
            });
        }
        if self.bar == 15 {
            return Some(Signal {
                action: SignalAction::CloseAll,
            });
        }
        None
    }
}

#[test]
fn golden_options_fixed_straddle() {
    let df = load("tests/fixtures/eq_ohlcv.csv");
    let mut strat = FixedStraddle { bar: 0 };
    strat.init(&HashMap::new());
    // Naked-sell margin floor is 150_000/lot (SEBI floor path in handle_open_position);
    // 100_000 capital would round quantity to 0 and never open a position, so this
    // test uses a larger capital base than the equity golden test.
    let mut engine =
        BacktestEngine::new(300_000.0).with_fill_model(Box::new(vajra::fill::SameBarClose));
    let trades = engine.run(&df, &mut strat).unwrap();
    assert_eq!(trades.len(), 1);
    let pnl = trades[0].pnl.unwrap();
    assert!((pnl - -1244.091014149768).abs() < 1e-6, "pnl was {pnl}"); // capture-then-freeze
}

#[test]
fn golden_options_next_bar_open() {
    let df = load("tests/fixtures/eq_ohlcv.csv");
    let mut strat = FixedStraddle { bar: 0 };
    strat.init(&HashMap::new());
    // Default fill model is NextBarOpen; assert the number DIFFERS from the
    // SameBarClose golden (-1244.091...), proving lookahead was removed.
    let mut engine = BacktestEngine::new(300_000.0);
    let trades = engine.run(&df, &mut strat).unwrap();
    assert_eq!(trades.len(), 1);
    let pnl = trades[0].pnl.unwrap();
    assert!(
        (pnl - -1244.091014149768).abs() > 1e-6,
        "expected a different fill than SameBarClose, got {pnl}"
    );
    // CAPTURE-THEN-FREEZE: captured and frozen.
    assert!((pnl - -1253.8012142416735).abs() < 1e-6, "pnl was {pnl}");
}

// A strategy that opens on the LAST bar cannot fill (no t+1) — position stays open.
struct OpenOnLastBar {
    n: usize,
    i: usize,
}
impl Strategy for OpenOnLastBar {
    fn init(&mut self, _p: &HashMap<String, serde_json::Value>) {}
    fn on_bar(
        &mut self,
        _ts: i64,
        _o: f64,
        _h: f64,
        _l: f64,
        c: f64,
        _v: f64,
        _pnl: Option<f64>,
    ) -> Option<Signal> {
        self.i += 1;
        if self.i == self.n {
            let atm = (c / 100.0).round() * 100.0;
            return Some(Signal {
                action: SignalAction::OpenPosition {
                    legs: vec![OptionLeg {
                        strike: atm,
                        option_type: "CE".into(),
                        entry_price: 0.0,
                        exit_price: None,
                        action: "SELL".into(),
                        expiry_days: Some(7.0),
                    }],
                    expiry_days: Some(7.0),
                },
            });
        }
        None
    }
}

#[test]
fn next_bar_open_has_no_lookahead() {
    // A decision on the final bar has no next bar to fill at: it must NOT fill
    // at that bar's own close (which would be lookahead). No trade is recorded.
    let df = load("tests/fixtures/eq_ohlcv.csv");
    let n = df.height();
    let mut strat = OpenOnLastBar { n, i: 0 };
    strat.init(&HashMap::new());
    let mut engine = BacktestEngine::new(300_000.0); // NextBarOpen default
    let trades = engine.run(&df, &mut strat).unwrap();
    // Position opened on the last bar can't fill forward; nothing closes => no completed trade.
    assert_eq!(
        trades.len(),
        0,
        "last-bar decision must not fill via lookahead"
    );
}
