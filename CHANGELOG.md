# Changelog

## 0.1.0 — Initial extraction from rust-algo

First public release. Extracted the backtesting core from a private trading
stack into a standalone, dependency-light crate.

- **Engine**: bar-driven `BacktestEngine` with a `Strategy` trait; supports
  single-leg `Buy`/`Sell` and multi-leg option `OpenPosition`/`CloseAll` signals.
- **Greeks**: built-in Black-Scholes pricing and delta/gamma/vega/theta.
- **Costs**: `CostModel` trait with `ZeroCost` (frictionless) and
  `IndiaOptionsCost` (brokerage + GST + STT + slippage); swap via
  `BacktestEngine::with_cost`.
- **Data**: path-driven `CsvDataLoader::from_path` over Polars.
- **Reporting**: HTML performance report + machine-readable `write_trades_csv`.
- **Strategies**: reference `MovingAverageCrossover`.

Runtime-free by default (no async dependencies).
