use crate::greeks::calculate_greeks;
use anyhow::Result;
use polars::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OptionLeg {
    pub strike: f64,
    pub option_type: String, // "CE", "PE", "EQ"
    pub entry_price: f64,
    pub exit_price: Option<f64>,
    pub action: String, // "BUY", "SELL"
    pub expiry_days: Option<f64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestConfig {
    pub strategy: String,
    pub symbol: String,
    pub timeframe: String,
    pub start_date: String,
    pub end_date: String,
    pub parameters: HashMap<String, serde_json::Value>,
    pub initial_capital: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinancialParams {
    pub risk_free_rate: f64,
    pub iv_assumption: f64,
    pub days_to_expiry: f64,
    pub lot_size: f64,
    pub margin_multiplier: f64,
}

impl Default for FinancialParams {
    fn default() -> Self {
        Self {
            risk_free_rate: 0.07,
            iv_assumption: 0.20,
            days_to_expiry: 7.0,
            lot_size: 50.0,
            margin_multiplier: 2.0,
        }
    }
}
#[derive(Debug, Clone)]
pub struct Trade {
    pub symbol: String,
    pub entry_time: i64,
    pub exit_time: Option<i64>,
    pub entry_price: f64, // For underlying or net premium
    pub exit_price: Option<f64>,
    pub quantity: i32,
    pub pnl: Option<f64>,
    pub direction: String, // "LONG", "SHORT", or "SPREAD"
    pub legs: Vec<OptionLeg>,
}

pub trait Strategy: Send {
    fn init(&mut self, params: &HashMap<String, serde_json::Value>);
    #[allow(clippy::too_many_arguments)]
    fn on_bar(
        &mut self,
        timestamp: i64,
        open: f64,
        high: f64,
        low: f64,
        close: f64,
        volume: f64,
        current_pnl: Option<f64>,
    ) -> Option<Signal>;
}

#[derive(Debug, Clone, PartialEq)]
pub enum SignalAction {
    Buy,
    Sell,
    OpenPosition {
        legs: Vec<OptionLeg>,
        expiry_days: Option<f64>,
    },
    CloseAll,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Signal {
    pub action: SignalAction,
}

pub struct BacktestEngine {
    pub capital: f64,
    pub params: FinancialParams,
    position: Option<Trade>,
    trades: Vec<Trade>,
    cost: Box<dyn crate::cost::CostModel>,
}

impl BacktestEngine {
    pub fn new(initial_capital: f64) -> Self {
        Self {
            capital: initial_capital,
            params: FinancialParams::default(),
            position: None,
            trades: Vec::new(),
            cost: Box::new(crate::cost::IndiaOptionsCost),
        }
    }

    pub fn with_cost(initial_capital: f64, cost: Box<dyn crate::cost::CostModel>) -> Self {
        Self {
            capital: initial_capital,
            params: FinancialParams::default(),
            position: None,
            trades: Vec::new(),
            cost,
        }
    }

    pub fn with_params(mut self, params: FinancialParams) -> Self {
        self.params = params;
        self
    }

    pub fn run(&mut self, df: &DataFrame, strategy: &mut dyn Strategy) -> Result<Vec<Trade>> {
        let closes = df.column("close")?.f64()?;
        let opens = df.column("open")?.f64()?;
        let highs = df.column("high")?.f64()?;
        let lows = df.column("low")?.f64()?;
        let volumes_series = df.column("volume")?.cast(&DataType::Float64)?;
        let volumes = volumes_series.f64()?;
        let timestamps = df.column("timestamp")?.str()?;

        for i in 0..closes.len() {
            let close = closes.get(i).unwrap();
            let open = opens.get(i).unwrap();
            let high = highs.get(i).unwrap();
            let low = lows.get(i).unwrap();
            let volume = volumes.get(i).unwrap();
            let ts_str = timestamps.get(i).unwrap();

            let dt = chrono::NaiveDateTime::parse_from_str(ts_str, "%Y-%m-%d %H:%M:%S")
                .unwrap_or_default();
            let timestamp = dt.and_utc().timestamp();

            // Calculate current unrealized PnL if a position is open
            let current_pnl = if let Some(ref trade) = self.position {
                let _pnl = 0.0;
                if trade.direction == "SPREAD" {
                    let mut current_value = 0.0;

                    for leg in &trade.legs {
                        let greeks_price = if leg.option_type == "EQ" {
                            close
                        } else {
                            let is_call = leg.option_type == "CE";
                            use chrono::Datelike;
                            let dt = chrono::DateTime::from_timestamp(timestamp, 0)
                                .unwrap_or_default()
                                .naive_utc();
                            let mut days_to_expiry = 0.0;
                            let mut current_date = dt.date();
                            while current_date.weekday() != chrono::Weekday::Thu {
                                current_date = current_date.succ_opt().unwrap();
                                days_to_expiry += 1.0;
                            }
                            if days_to_expiry == 0.0 {
                                days_to_expiry = 0.1;
                            }

                            let t = days_to_expiry / 365.0;
                            let greeks = calculate_greeks(
                                close,
                                leg.strike,
                                t,
                                self.params.risk_free_rate,
                                self.params.iv_assumption,
                                is_call,
                            );
                            greeks.price
                        };

                        if leg.action == "SELL" {
                            current_value += greeks_price;
                        } else {
                            current_value -= greeks_price;
                        }
                    }
                    Some(
                        (trade.entry_price - current_value)
                            * trade.quantity as f64
                            * self.params.lot_size,
                    )
                } else {
                    Some((close - trade.entry_price) * trade.quantity as f64)
                }
            } else {
                None
            };

            if let Some(signal) =
                strategy.on_bar(timestamp, open, high, low, close, volume, current_pnl)
            {
                self.process_signal(signal, timestamp, close);
            }
        }

        Ok(self.trades.clone())
    }

    fn process_signal(&mut self, signal: Signal, timestamp: i64, price: f64) {
        match signal.action {
            SignalAction::Buy => self.handle_buy(timestamp, price),
            SignalAction::Sell => self.handle_sell(timestamp, price),
            SignalAction::OpenPosition {
                legs,
                expiry_days: _,
            } => self.handle_open_position(timestamp, price, legs),
            SignalAction::CloseAll => self.handle_close_all(timestamp, price),
        }
    }

    fn handle_buy(&mut self, timestamp: i64, price: f64) {
        if self.position.is_none() {
            let quantity = (self.capital / price) as i32;
            if quantity > 0 {
                self.position = Some(Trade {
                    symbol: "TEST".to_string(),
                    entry_time: timestamp,
                    exit_time: None,
                    entry_price: price,
                    exit_price: None,
                    quantity,
                    pnl: None,
                    direction: "LONG".to_string(),
                    legs: Vec::new(),
                });
            }
        }
    }

    fn handle_sell(&mut self, timestamp: i64, price: f64) {
        if let Some(mut trade) = self.position.take() {
            if trade.direction == "LONG" {
                trade.exit_price = Some(price);
                trade.exit_time = Some(timestamp);
                let pnl = (price - trade.entry_price) * trade.quantity as f64;
                trade.pnl = Some(pnl);
                self.capital += pnl;
                self.trades.push(trade);
            } else {
                self.position = Some(trade); // Put it back
            }
        }
    }

    fn handle_open_position(&mut self, timestamp: i64, price: f64, signal_legs: Vec<OptionLeg>) {
        if self.position.is_none() {
            let mut total_net_premium = 0.0f64;
            let mut total_margin: f64 = 0.0f64;
            let mut all_legs: Vec<OptionLeg> = Vec::new();

            for signal_leg in signal_legs {
                let mut entry_price = if signal_leg.option_type == "EQ" {
                    price
                } else {
                    let is_call = signal_leg.option_type == "CE";
                    use chrono::Datelike;
                    let dt = chrono::DateTime::from_timestamp(timestamp, 0)
                        .unwrap_or_default()
                        .naive_utc();
                    let mut days_to_expiry = 0.0;
                    let mut current_date = dt.date();
                    while current_date.weekday() != chrono::Weekday::Thu {
                        current_date = current_date.succ_opt().unwrap();
                        days_to_expiry += 1.0;
                    }
                    if days_to_expiry == 0.0 {
                        days_to_expiry = 0.1;
                    } // 0 DTE

                    let t = days_to_expiry / 365.0;
                    let r = self.params.risk_free_rate;
                    let sigma = self.params.iv_assumption;

                    let greeks = calculate_greeks(price, signal_leg.strike, t, r, sigma, is_call);
                    greeks.price
                };

                entry_price = self.cost.adjust_fill(
                    entry_price,
                    &signal_leg.action,
                    signal_leg.option_type != "EQ",
                    false,
                );

                let calculated_leg = OptionLeg {
                    strike: signal_leg.strike,
                    option_type: signal_leg.option_type,
                    entry_price,
                    exit_price: None,
                    action: signal_leg.action,
                    expiry_days: signal_leg.expiry_days,
                };
                all_legs.push(calculated_leg.clone());

                if calculated_leg.action == "SELL" {
                    total_net_premium += entry_price;
                    // For margin calculation, approximate with largest difference for now
                    // More precise margin would sum margin for each spread if it's an Iron Condor
                    total_margin = total_margin.max(
                        signal_leg.strike * self.params.lot_size * self.params.margin_multiplier,
                    );
                } else {
                    total_net_premium -= entry_price;
                }
            }

            // Correct Margin Calculation for Iron Condor or single Spreads
            let ce_spread = all_legs
                .iter()
                .find(|leg| leg.option_type == "CE" && leg.action == "BUY")
                .map(|long_leg| {
                    let short_leg = all_legs
                        .iter()
                        .find(|l| l.option_type == "CE" && l.action == "SELL")
                        .unwrap();
                    (long_leg.strike - short_leg.strike).abs()
                })
                .unwrap_or(0.0);

            let pe_spread = all_legs
                .iter()
                .find(|leg| leg.option_type == "PE" && leg.action == "BUY")
                .map(|long_leg| {
                    let short_leg = all_legs
                        .iter()
                        .find(|l| l.option_type == "PE" && l.action == "SELL")
                        .unwrap();
                    (long_leg.strike - short_leg.strike).abs()
                })
                .unwrap_or(0.0);

            let spread_width = ce_spread.max(pe_spread);
            let margin_per_lot = if spread_width > 0.0 {
                spread_width * self.params.lot_size
            } else if all_legs.iter().any(|l| l.option_type == "EQ") {
                price * self.params.lot_size // Full value for equity
            } else {
                150_000.0 // SEBI floor for naked selling
            };

            let quantity = if margin_per_lot > 0.0 {
                (self.capital / margin_per_lot) as i32
            } else {
                1
            };

            println!(
                "handle_open_position: capital={}, net_premium={}, margin_per_lot={}, quantity={}",
                self.capital, total_net_premium, margin_per_lot, quantity
            );

            if quantity > 0 {
                self.position = Some(Trade {
                    symbol: "NIFTY".to_string(), // Assuming Nifty for now
                    entry_time: timestamp,
                    exit_time: None,
                    entry_price: total_net_premium,
                    exit_price: None,
                    quantity,
                    pnl: None,
                    direction: "SPREAD".to_string(),
                    legs: all_legs,
                });
                println!(
                    "Position opened successfully. Current position: {:?}",
                    self.position
                );
            }
        }
    }

    fn handle_close_all(&mut self, timestamp: i64, price: f64) {
        println!(
            "handle_close_all: timestamp={}, price={}, position_is_some={}",
            timestamp,
            price,
            self.position.is_some()
        );
        if let Some(mut trade) = self.position.take() {
            println!("Closing position: {:?}", trade.symbol);
            if trade.direction == "SPREAD" {
                let mut current_net_premium = 0.0;

                for leg in &mut trade.legs {
                    let mut exit_price = if leg.option_type == "EQ" {
                        price
                    } else {
                        let is_call = leg.option_type == "CE";
                        use chrono::Datelike;
                        let dt = chrono::DateTime::from_timestamp(timestamp, 0)
                            .unwrap_or_default()
                            .naive_utc();
                        let mut days_to_expiry = 0.0;
                        let mut current_date = dt.date();
                        while current_date.weekday() != chrono::Weekday::Thu {
                            current_date = current_date.succ_opt().unwrap();
                            days_to_expiry += 1.0;
                        }
                        if days_to_expiry == 0.0 {
                            days_to_expiry = 0.1;
                        } // 0 DTE

                        let t = days_to_expiry / 365.0;
                        let r = self.params.risk_free_rate;
                        let sigma = self.params.iv_assumption;
                        let greeks = calculate_greeks(price, leg.strike, t, r, sigma, is_call);
                        greeks.price
                    };

                    // Exit Slippage
                    if leg.option_type == "EQ" {
                        // vajra: preserves double-slippage bug from source; fix post-extraction
                        exit_price = self.cost.adjust_fill(exit_price, &leg.action, false, true);
                        exit_price = self.cost.adjust_fill(exit_price, &leg.action, false, true);
                    } else {
                        exit_price = self.cost.adjust_fill(exit_price, &leg.action, true, true);
                    }

                    leg.exit_price = Some(exit_price);

                    if leg.action == "SELL" {
                        current_net_premium += exit_price;
                    } else {
                        current_net_premium -= exit_price;
                    }
                }

                trade.exit_price = Some(current_net_premium);
                trade.exit_time = Some(timestamp);
                let gross_pnl = (trade.entry_price - current_net_premium)
                    * trade.quantity as f64
                    * self.params.lot_size;

                // Friction
                let costs = self.cost.round_trip_cost(&trade, self.params.lot_size);
                let net_pnl = gross_pnl - costs;

                trade.pnl = Some(net_pnl);
                self.capital += net_pnl;
                self.trades.push(trade);
                println!("Trade closed and pushed. PnL: {}", net_pnl);
            } else {
                // handle non-spread trades
                trade.exit_price = Some(price);
                trade.exit_time = Some(timestamp);
                let pnl = (price - trade.entry_price) * trade.quantity as f64;
                trade.pnl = Some(pnl);
                self.capital += pnl;
                self.trades.push(trade);
                println!("Non-spread trade closed and pushed. PnL: {}", pnl);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::MovingAverageCrossover;
    use serde_json::json;

    #[test]
    fn test_ma_crossover_logic() {
        let mut strategy = MovingAverageCrossover::new();
        let params = vec![
            ("short_window".to_string(), json!(2)),
            ("long_window".to_string(), json!(5)),
        ]
        .into_iter()
        .collect();

        strategy.init(&params);

        assert!(strategy.on_bar(0, 0.0, 0.0, 0.0, 10.0, 0.0, None).is_none());
        assert!(strategy.on_bar(0, 0.0, 0.0, 0.0, 11.0, 0.0, None).is_none());

        let signal = strategy.on_bar(0, 0.0, 0.0, 0.0, 12.0, 0.0, None);
        assert_eq!(
            signal,
            Some(Signal {
                action: SignalAction::Buy
            })
        );
    }

    #[test]
    fn test_spread_lifecycle() {
        let mut engine = BacktestEngine::new(100000.0);
        let timestamp = 1715856000; // Example unix ts
        let price = 22000.0;

        // 1. Open Spread
        engine.process_signal(
            Signal {
                action: SignalAction::OpenPosition {
                    legs: vec![
                        OptionLeg {
                            strike: 22100.0,
                            option_type: "CE".to_string(),
                            entry_price: 0.0,
                            exit_price: None,
                            action: "SELL".to_string(),
                            expiry_days: None,
                        },
                        OptionLeg {
                            strike: 22200.0,
                            option_type: "CE".to_string(),
                            entry_price: 0.0,
                            exit_price: None,
                            action: "BUY".to_string(),
                            expiry_days: None,
                        },
                    ],
                    expiry_days: None,
                },
            },
            timestamp,
            price,
        );

        assert!(engine.position.is_some());
        let pos = engine.position.as_ref().unwrap();
        assert_eq!(pos.direction, "SPREAD");
        assert_eq!(pos.legs.len(), 2);

        // 2. Close All
        engine.process_signal(
            Signal {
                action: SignalAction::CloseAll,
            },
            timestamp + 3600,
            price - 50.0,
        );

        assert!(engine.position.is_none());
        assert_eq!(engine.trades.len(), 1);
        let trade = &engine.trades[0];
        assert!(trade.pnl.is_some());
        // Since price dropped, CE spread should be profitable
        assert!(trade.pnl.unwrap() > 0.0);
    }
}
