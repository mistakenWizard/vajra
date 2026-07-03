use std::collections::HashMap;
use vajra::data::CsvDataLoader;
use vajra::engine::{BacktestEngine, Strategy};
use vajra::strategies::MovingAverageCrossover;

fn main() -> anyhow::Result<()> {
    let df = CsvDataLoader::new().from_path("tests/fixtures/eq_ohlcv.csv")?;
    let mut strat = MovingAverageCrossover::new();
    strat.init(&HashMap::new());
    let trades = BacktestEngine::new(100_000.0).run(&df, &mut strat)?;
    println!("trades: {}", trades.len());
    Ok(())
}
