# Changelog

## [Unreleased]

### Changed

- **Default fill is now lookahead-free** (`NextBarOpen`, t+1 open). Opt into the
  old same-bar-close behavior with `BacktestEngine::with_fill_model(Box::new(SameBarClose))`.

### Added

- `FillModel` trait with `NextBarOpen` / `SameBarClose`.
- Real per-leg option marking via `OptionQuoteSource` / `MapOptionSource`;
  Black-Scholes is now an explicit, counted fallback (`BacktestEngine::modeled_fills()`).
- Spread-aware slippage (`CostModel::adjust_fill_spread`) using quoted bid/ask.

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
