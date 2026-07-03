# Vajra

**A lean, fast, options-native backtesting engine for Rust.**

Most Rust backtesters are equity-first: a strategy emits buy/sell on a single
price series. Vajra treats **multi-leg option positions as first-class** — it
prices legs with a built-in Black-Scholes greeks engine, models Indian F&O
frictions (brokerage, GST, STT, slippage) behind a swappable cost model, and
runs the whole thing over a Polars `DataFrame` of OHLCV bars.

- **Options-native**: open/close multi-leg CE/PE positions, priced per-bar with greeks.
- **Greeks built in**: Black-Scholes price + delta/gamma/vega/theta, no external service.
- **Pluggable costs**: `ZeroCost` for frictionless research, `IndiaOptionsCost` for realistic F&O fills — or implement `CostModel` yourself.
- **Fast core, no runtime**: pure synchronous, Polars-backed. No tokio/reqwest in the default build.

## Quickstart

```rust
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
```

Run it:

```bash
cargo run --example quickstart
```

Write your own strategy by implementing the `Strategy` trait (`on_bar` returns
an optional `Signal` — `Buy`, `Sell`, `OpenPosition { legs, expiry_days }`, or
`CloseAll`). Plug a custom `CostModel` into `BacktestEngine::with_cost` to model
your own venue's frictions.

### Implied volatility

Option legs are priced with Black-Scholes, so they need a volatility input. By
default the engine uses a single flat assumption (override it via
`BacktestEngine::new(capital).with_params(FinancialParams { iv_assumption, .. })`).
For realistic pricing, add an `iv` column to your bar `DataFrame` — the engine
reads per-bar implied vol from it and only falls back to the flat assumption
when the column is absent.

## Status

**v0.1 — API unstable.** Extracted from a private trading stack and still
settling; expect breaking changes across 0.x. Pin an exact version.

## License

Licensed under either of Apache-2.0 or MIT at your option.
