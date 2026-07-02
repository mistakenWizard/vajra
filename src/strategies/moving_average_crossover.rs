use crate::engine::{Signal, SignalAction, Strategy};
use std::collections::HashMap;
use ta::indicators::SimpleMovingAverage;
use ta::Next;

pub struct MovingAverageCrossover {
    short_ma: Box<dyn Next<f64, Output = f64> + Send>,
    long_ma: Box<dyn Next<f64, Output = f64> + Send>,
    short_val: f64,
    long_val: f64,
    prev_short: f64,
    prev_long: f64,
}

impl MovingAverageCrossover {
    pub fn new() -> Self {
        Self {
            short_ma: Box::new(SimpleMovingAverage::new(10).unwrap()),
            long_ma: Box::new(SimpleMovingAverage::new(50).unwrap()),
            short_val: 0.0,
            long_val: 0.0,
            prev_short: 0.0,
            prev_long: 0.0,
        }
    }
}

impl Default for MovingAverageCrossover {
    fn default() -> Self {
        Self::new()
    }
}

impl Strategy for MovingAverageCrossover {
    fn init(&mut self, params: &HashMap<String, serde_json::Value>) {
        let short_window = params
            .get("short_window")
            .and_then(|v| v.as_u64())
            .unwrap_or(20) as usize;
        let long_window = params
            .get("long_window")
            .and_then(|v| v.as_u64())
            .unwrap_or(50) as usize;

        self.short_ma = Box::new(SimpleMovingAverage::new(short_window).unwrap());
        self.long_ma = Box::new(SimpleMovingAverage::new(long_window).unwrap());
    }

    fn on_bar(
        &mut self,
        _timestamp: i64,
        _open: f64,
        _high: f64,
        _low: f64,
        close: f64,
        _volume: f64,
        _current_pnl: Option<f64>,
    ) -> Option<Signal> {
        self.prev_short = self.short_val;
        self.prev_long = self.long_val;

        self.short_val = self.short_ma.next(close);
        self.long_val = self.long_ma.next(close);

        if self.short_val == 0.0 || self.long_val == 0.0 {
            return None;
        }

        if self.prev_short <= self.prev_long && self.short_val > self.long_val {
            Some(Signal {
                action: SignalAction::Buy,
            })
        } else if self.prev_short >= self.prev_long && self.short_val < self.long_val {
            Some(Signal {
                action: SignalAction::Sell,
            })
        } else {
            None
        }
    }
}
