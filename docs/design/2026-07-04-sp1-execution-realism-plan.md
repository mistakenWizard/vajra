# SP1 — Execution Realism & Real-Quote Marking: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give vajra lookahead-free fills and real per-leg option-quote marking, with Black-Scholes demoted to an explicit, counted fallback.

**Architecture:** A narrow `OptionQuoteSource` overlay supplies real CE/PE quotes (keyed by bar timestamp); a `FillModel` decides *which bar and which price* a decision fills at; the engine defers fills to the model's bar (no same-bar lookahead by default) and prices each leg from a real quote when present, else BS (incrementing a `modeled_fills` counter surfaced on the engine). Existing goldens are pinned under an explicit `SameBarClose` model so they stay bit-identical; new goldens capture the `NextBarOpen` default and real-quote marking.

**Tech Stack:** Rust 2021, polars 0.36, chrono, existing `calculate_greeks`. No new runtime deps.

## Global Constraints

- Crate stays **runtime-free** in the default build (no tokio/reqwest). One line each, verbatim from repo conventions:
- `cargo test` green; `cargo test --no-default-features` green; `cargo clippy --all-targets -- -D warnings` clean; `cargo fmt --check` clean.
- **No `shared` / `/mnt/e` / `Dhan` / `Telegram` / `NSE` references** in crate source.
- The deliberate **double-EQ-slippage** in `handle_close_all` (marked `// vajra: preserves double-slippage bug...`) stays untouched.
- Golden P&L values are frozen: equity `15531.140700000002`, options `-1244.091014149768` — must remain exact under `SameBarClose`.

## Deviations from the SP1 spec (flagged for review)

1. `PriceSource` (with `underlying()`) → **`OptionQuoteSource`** (option-quote overlay only). The bar df already carries underlying OHLC/volume/iv; a second underlying source would be redundant state to keep in sync. Same intent: real option quotes in, BS as fallback.
2. `FillModel::resolve(decision_i, side, quote) -> Fill` is not implementable as written (it needs the fill bar's quote to decide the fill bar — chicken/egg). Split into `fill_bar(decision_i)` + `underlying_field()` + `quote_price(&Quote)`. `Side` enum dropped (only `WorstOfBar` needed it).
3. **Vwap / WorstOfBar** fill models and the **minimal expiry seam** are cut from SP1. Vwap/WorstOfBar are speculative (YAGNI). The expiry seam conflicts with bit-identical `SameBarClose` goldens (the pinned straddle already carries `expiry_days: 7.0` the engine ignores) → moves wholly to SP3.

## File structure

- Create `src/marketdata.rs` — `Quote`, `InstrumentKey`, `OptionQuoteSource` trait, `MapOptionSource` (in-memory + long-df loader).
- Create `src/fill.rs` — `BarField`, `FillModel` trait, `NextBarOpen`, `SameBarClose`.
- Modify `src/cost.rs` — add default `adjust_fill_spread`.
- Modify `src/engine.rs` — new fields/builders, deferred-fill `run()` loop, per-leg real-vs-modeled pricing, `modeled_fills()`.
- Modify `src/lib.rs` — `pub mod marketdata; pub mod fill;`.
- Modify `tests/golden.rs` — pin both goldens to `SameBarClose`; add `NextBarOpen` + real-quote goldens.
- Create `tests/fixtures/opt_long.csv` — synthetic long-format option quotes for the real-quote test.
- Modify `README.md` / `CHANGELOG.md` — document default fill change + real-quote marking.

---

### Task 1: Market-data types + `OptionQuoteSource` trait

**Files:**
- Create: `src/marketdata.rs`
- Modify: `src/lib.rs`
- Test: inline `#[cfg(test)]` in `src/marketdata.rs`

**Interfaces:**
- Produces: `Quote { open: f64, high: f64, low: f64, close: f64, bid: Option<f64>, ask: Option<f64> }` with `fn spread(&self) -> Option<f64>`; `InstrumentKey { strike: f64, option_type: String }` (Hash+Eq via `strike.to_bits()`); `trait OptionQuoteSource { fn option_quote(&self, key: &InstrumentKey, bar_ts: i64) -> Option<Quote>; }`.

- [ ] **Step 1: Write the failing test**

```rust
// bottom of src/marketdata.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_spread_present_and_absent() {
        let q = Quote { open: 10.0, high: 11.0, low: 9.0, close: 10.5, bid: Some(10.0), ask: Some(10.4) };
        assert!((q.spread().unwrap() - 0.4).abs() < 1e-9);
        let q2 = Quote { bid: None, ask: None, ..q };
        assert!(q2.spread().is_none());
    }

    #[test]
    fn instrument_key_hashes_by_value() {
        use std::collections::HashMap;
        let mut m = HashMap::new();
        m.insert(InstrumentKey { strike: 22100.0, option_type: "CE".into() }, 1);
        assert_eq!(m.get(&InstrumentKey { strike: 22100.0, option_type: "CE".into() }), Some(&1));
        assert_eq!(m.get(&InstrumentKey { strike: 22200.0, option_type: "CE".into() }), None);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p vajra --lib marketdata`
Expected: FAIL — `src/marketdata.rs` / module not found.

- [ ] **Step 3: Write minimal implementation**

```rust
// top of src/marketdata.rs
//! Market-data types and the option-quote overlay used for real-quote marking.

/// A real option (or underlying) quote at one bar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quote {
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub bid: Option<f64>,
    pub ask: Option<f64>,
}

impl Quote {
    /// Quoted spread `(ask - bid)` when both sides are present.
    pub fn spread(&self) -> Option<f64> {
        match (self.bid, self.ask) {
            (Some(b), Some(a)) => Some(a - b),
            _ => None,
        }
    }
}

/// Identifies an option leg for quote lookup. Expiry is deferred to SP3.
#[derive(Debug, Clone)]
pub struct InstrumentKey {
    pub strike: f64,
    pub option_type: String, // "CE" / "PE"
}

impl PartialEq for InstrumentKey {
    fn eq(&self, other: &Self) -> bool {
        self.strike.to_bits() == other.strike.to_bits() && self.option_type == other.option_type
    }
}
impl Eq for InstrumentKey {}
impl std::hash::Hash for InstrumentKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.strike.to_bits().hash(state);
        self.option_type.hash(state);
    }
}

/// Supplies real option quotes when a series exists for the instrument at a bar.
/// `None` ⇒ the engine falls back to Black-Scholes model pricing (counted).
pub trait OptionQuoteSource: Send {
    fn option_quote(&self, key: &InstrumentKey, bar_ts: i64) -> Option<Quote>;
}
```

Add to `src/lib.rs` after `pub mod greeks;`:

```rust
pub mod marketdata;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p vajra --lib marketdata`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add src/marketdata.rs src/lib.rs
git commit -m "feat(marketdata): Quote, InstrumentKey, OptionQuoteSource trait"
```

---

### Task 2: `MapOptionSource` — in-memory source + long-format CSV loader

**Files:**
- Modify: `src/marketdata.rs`
- Test: inline `#[cfg(test)]` in `src/marketdata.rs`; fixture `tests/fixtures/opt_long.csv`

**Interfaces:**
- Consumes: `Quote`, `InstrumentKey`, `OptionQuoteSource` (Task 1).
- Produces: `MapOptionSource` with `fn new() -> Self`, `fn insert(&mut self, bar_ts: i64, key: InstrumentKey, q: Quote)`, and `fn from_long_df(df: &polars::prelude::DataFrame) -> anyhow::Result<Self>`. Long-format columns: `timestamp` (str `%Y-%m-%d %H:%M:%S`), `strike` (f64), `option_type` (str), `open,high,low,close` (f64), optional `bid,ask` (f64). Implements `OptionQuoteSource`.

- [ ] **Step 1: Write the failing test**

```rust
// add inside the existing #[cfg(test)] mod tests in src/marketdata.rs
    #[test]
    fn map_source_returns_inserted_quote() {
        let mut s = MapOptionSource::new();
        let key = InstrumentKey { strike: 22100.0, option_type: "CE".into() };
        let q = Quote { open: 5.0, high: 6.0, low: 4.0, close: 5.5, bid: None, ask: None };
        s.insert(1000, key.clone(), q);
        assert_eq!(s.option_quote(&key, 1000), Some(q));
        assert_eq!(s.option_quote(&key, 2000), None); // wrong ts
        let pe = InstrumentKey { strike: 22100.0, option_type: "PE".into() };
        assert_eq!(s.option_quote(&pe, 1000), None); // wrong type
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p vajra --lib marketdata::tests::map_source_returns_inserted_quote`
Expected: FAIL — `MapOptionSource` not found.

- [ ] **Step 3: Write minimal implementation**

```rust
// append to src/marketdata.rs (above the tests module)
use std::collections::HashMap;

/// In-memory `OptionQuoteSource` keyed by `(bar_ts, InstrumentKey)`.
#[derive(Default)]
pub struct MapOptionSource {
    quotes: HashMap<(i64, InstrumentKey), Quote>,
}

impl MapOptionSource {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, bar_ts: i64, key: InstrumentKey, q: Quote) {
        self.quotes.insert((bar_ts, key), q);
    }

    /// Build from a long-format option DataFrame (one row per bar×strike×type).
    pub fn from_long_df(df: &polars::prelude::DataFrame) -> anyhow::Result<Self> {
        use polars::prelude::*;
        let ts = df.column("timestamp")?.str()?;
        let strike = df.column("strike")?.f64()?;
        let otype = df.column("option_type")?.str()?;
        let open = df.column("open")?.f64()?;
        let high = df.column("high")?.f64()?;
        let low = df.column("low")?.f64()?;
        let close = df.column("close")?.f64()?;
        let bid = df.column("bid").ok().and_then(|c| c.f64().ok());
        let ask = df.column("ask").ok().and_then(|c| c.f64().ok());

        let mut s = Self::new();
        for i in 0..df.height() {
            let dt = chrono::NaiveDateTime::parse_from_str(ts.get(i).unwrap(), "%Y-%m-%d %H:%M:%S")
                .unwrap_or_default();
            let bar_ts = dt.and_utc().timestamp();
            let key = InstrumentKey {
                strike: strike.get(i).unwrap(),
                option_type: otype.get(i).unwrap().to_string(),
            };
            let q = Quote {
                open: open.get(i).unwrap(),
                high: high.get(i).unwrap(),
                low: low.get(i).unwrap(),
                close: close.get(i).unwrap(),
                bid: bid.and_then(|c| c.get(i)),
                ask: ask.and_then(|c| c.get(i)),
            };
            s.insert(bar_ts, key, q);
        }
        Ok(s)
    }
}

impl OptionQuoteSource for MapOptionSource {
    fn option_quote(&self, key: &InstrumentKey, bar_ts: i64) -> Option<Quote> {
        self.quotes.get(&(bar_ts, key.clone())).copied()
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p vajra --lib marketdata`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add src/marketdata.rs
git commit -m "feat(marketdata): MapOptionSource in-memory source + long-df loader"
```

---

### Task 3: `FillModel` trait + `NextBarOpen` / `SameBarClose`

**Files:**
- Create: `src/fill.rs`
- Modify: `src/lib.rs`
- Test: inline `#[cfg(test)]` in `src/fill.rs`

**Interfaces:**
- Consumes: `Quote` (Task 1).
- Produces: `enum BarField { Open, High, Low, Close }`; `trait FillModel { fn fill_bar(&self, decision_i: usize) -> usize; fn underlying_field(&self) -> BarField; fn quote_price(&self, q: &Quote) -> f64; }`; unit structs `NextBarOpen`, `SameBarClose`. `NextBarOpen`: `fill_bar = decision_i + 1`, `underlying_field = Open`, `quote_price = q.open`. `SameBarClose`: `fill_bar = decision_i`, `underlying_field = Close`, `quote_price = q.close`.

- [ ] **Step 1: Write the failing test**

```rust
// bottom of src/fill.rs
#[cfg(test)]
mod tests {
    use super::*;
    use crate::marketdata::Quote;

    fn q() -> Quote {
        Quote { open: 1.0, high: 2.0, low: 0.5, close: 1.5, bid: None, ask: None }
    }

    #[test]
    fn next_bar_open_fills_at_next_bar_open() {
        let m = NextBarOpen;
        assert_eq!(m.fill_bar(3), 4);
        assert!(matches!(m.underlying_field(), BarField::Open));
        assert_eq!(m.quote_price(&q()), 1.0);
    }

    #[test]
    fn same_bar_close_fills_at_this_bar_close() {
        let m = SameBarClose;
        assert_eq!(m.fill_bar(3), 3);
        assert!(matches!(m.underlying_field(), BarField::Close));
        assert_eq!(m.quote_price(&q()), 1.5);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p vajra --lib fill`
Expected: FAIL — module `fill` not found.

- [ ] **Step 3: Write minimal implementation**

```rust
// top of src/fill.rs
//! Fill models: decide which bar and which price a decision executes at.
//! The default (`NextBarOpen`) is lookahead-free; `SameBarClose` reproduces
//! pre-SP1 (v0.1) behavior exactly and is used to pin golden tests.

use crate::marketdata::Quote;

/// Which OHLC field of the fill bar to price an underlying/EQ leg from.
#[derive(Debug, Clone, Copy)]
pub enum BarField {
    Open,
    High,
    Low,
    Close,
}

/// Resolves the bar and price a decision fills at.
pub trait FillModel: Send {
    /// Bar index a decision made on `decision_i` fills at.
    fn fill_bar(&self, decision_i: usize) -> usize;
    /// Underlying OHLC field to synthesize/mark from at the fill bar.
    fn underlying_field(&self) -> BarField;
    /// Price to take from a real option `Quote` at the fill bar.
    fn quote_price(&self, q: &Quote) -> f64;
}

/// Lookahead-free default: a decision on bar `t` fills at `t+1`'s open.
pub struct NextBarOpen;
impl FillModel for NextBarOpen {
    fn fill_bar(&self, decision_i: usize) -> usize {
        decision_i + 1
    }
    fn underlying_field(&self) -> BarField {
        BarField::Open
    }
    fn quote_price(&self, q: &Quote) -> f64 {
        q.open
    }
}

/// Pre-SP1 behavior: fills on the decision bar at its close (has lookahead).
/// Opt-in only, so historical goldens can pin exact numbers.
pub struct SameBarClose;
impl FillModel for SameBarClose {
    fn fill_bar(&self, decision_i: usize) -> usize {
        decision_i
    }
    fn underlying_field(&self) -> BarField {
        BarField::Close
    }
    fn quote_price(&self, q: &Quote) -> f64 {
        q.close
    }
}
```

Add to `src/lib.rs` after `pub mod engine;`:

```rust
pub mod fill;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p vajra --lib fill`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add src/fill.rs src/lib.rs
git commit -m "feat(fill): FillModel trait + NextBarOpen (default) / SameBarClose"
```

---

### Task 4: Spread-aware slippage on `CostModel`

**Files:**
- Modify: `src/cost.rs`
- Test: inline `#[cfg(test)]` in `src/cost.rs`

**Interfaces:**
- Produces: default trait method `fn adjust_fill_spread(&self, mid: f64, entry_action: &str, is_option: bool, is_exit: bool, spread: Option<f64>) -> f64` on `CostModel`. Default ignores `spread` and delegates to `adjust_fill` (so external impls keep flat behavior). `IndiaOptionsCost` overrides: when `is_option` and `spread == Some(s)`, widen by `s * 0.25` in the adverse direction instead of the flat `0.5`; otherwise delegate.

- [ ] **Step 1: Write the failing test**

```rust
// bottom of src/cost.rs
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
        assert_eq!(c.adjust_fill_spread(10.0, "BUY", true, false, Some(4.0)), 10.0);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p vajra --lib cost`
Expected: FAIL — `adjust_fill_spread` not found.

- [ ] **Step 3: Write minimal implementation**

Add the default method to the `CostModel` trait (after `adjust_fill`):

```rust
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
```

Override it in `impl CostModel for IndiaOptionsCost` (add the method inside that impl):

```rust
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
                let adds = if is_exit { entry_action == "SELL" } else { entry_action == "BUY" };
                if adds {
                    mid + s * 0.25
                } else {
                    mid - s * 0.25
                }
            }
            _ => self.adjust_fill(mid, entry_action, is_option, is_exit),
        }
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p vajra --lib cost`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add src/cost.rs
git commit -m "feat(cost): spread-aware adjust_fill_spread (default flat, India widens)"
```

---

### Task 5: Engine — `FillModel` field, route fills through it, pin goldens to `SameBarClose`

This task adds the fill-model seam **without changing any numbers**: the engine
routes its existing fill through a `FillModel`, defaulting to `NextBarOpen`, and
the goldens are updated to pin `SameBarClose` so they stay bit-identical. The
deferred no-lookahead loop lands in Task 6; here `SameBarClose` still fills on
the decision bar, matching today.

**Files:**
- Modify: `src/engine.rs` (fields ~91-97, constructors ~99-123, `run` ~125-214, `process_signal` ~216, `handle_open_position` ~262, `handle_close_all` ~398)
- Modify: `tests/golden.rs:19` and `tests/golden.rs:89`
- Test: `tests/golden.rs` (existing two goldens, now pinned)

**Interfaces:**
- Consumes: `FillModel`, `NextBarOpen`, `SameBarClose`, `BarField` (Task 3).
- Produces: field `fill_model: Box<dyn crate::fill::FillModel>` (default `NextBarOpen`); builder `fn with_fill_model(mut self, m: Box<dyn crate::fill::FillModel>) -> Self`; field `modeled_fills: usize` + `pub fn modeled_fills(&self) -> usize`. `run`'s per-bar loop still drives fills on the decision bar for `SameBarClose`.

- [ ] **Step 1: Write the failing test**

Update both goldens to pin `SameBarClose` (the assertions/values are unchanged; only the engine construction changes):

```rust
// tests/golden.rs — golden_equity_ma_crossover, replace the engine line:
    let mut engine = BacktestEngine::new(100_000.0)
        .with_fill_model(Box::new(vajra::fill::SameBarClose));
```

```rust
// tests/golden.rs — golden_options_fixed_straddle, replace the engine line:
    let mut engine = BacktestEngine::new(300_000.0)
        .with_fill_model(Box::new(vajra::fill::SameBarClose));
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p vajra --test golden`
Expected: FAIL — `with_fill_model` not found (compile error).

- [ ] **Step 3: Write minimal implementation**

In `src/engine.rs`, extend the struct and constructors:

```rust
// add to imports at top:
use crate::fill::{BarField, FillModel, NextBarOpen};
```

```rust
// struct BacktestEngine — add fields:
pub struct BacktestEngine {
    pub capital: f64,
    pub params: FinancialParams,
    position: Option<Trade>,
    trades: Vec<Trade>,
    cost: Box<dyn crate::cost::CostModel>,
    fill_model: Box<dyn FillModel>,
    modeled_fills: usize,
}
```

Set the defaults in **both** `new` and `with_cost` (add the two fields to each struct literal):

```rust
            fill_model: Box::new(NextBarOpen),
            modeled_fills: 0,
```

Add the builder + getter (after `with_params`):

```rust
    pub fn with_fill_model(mut self, m: Box<dyn FillModel>) -> Self {
        self.fill_model = m;
        self
    }

    /// How many option legs were priced via the Black-Scholes fallback
    /// (i.e. had no real quote) across the last `run`.
    pub fn modeled_fills(&self) -> usize {
        self.modeled_fills
    }
```

For this task the fill still happens on the decision bar. Thread the model's
`underlying_field` into the price the engine uses so `SameBarClose` (Close) is
identical to today. In `run`, after computing `open/high/low/close`, choose the
fill-bar underlying price by field and pass it as `price` into `process_signal`:

```rust
            // Underlying price the fill model marks against (SameBarClose => close, today's behavior).
            let fill_px = match self.fill_model.underlying_field() {
                BarField::Open => open,
                BarField::High => high,
                BarField::Low => low,
                BarField::Close => close,
            };
```

Change the `process_signal` call to pass `fill_px` (and keep `close` for the
`current_pnl` mark, which is separate and unchanged):

```rust
            if let Some(signal) =
                strategy.on_bar(timestamp, open, high, low, close, volume, current_pnl)
            {
                self.process_signal(signal, timestamp, fill_px, iv);
            }
```

Under `SameBarClose`, `underlying_field()` is `Close`, so `fill_px == close` and
every downstream number is unchanged.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p vajra --test golden`
Expected: PASS (2 tests, values `15531.140700000002` and `-1244.091014149768` unchanged).

- [ ] **Step 5: Run the full lib + no-default-features check**

Run: `cargo test -p vajra --lib && cargo test -p vajra --no-default-features && cargo clippy -p vajra --all-targets -- -D warnings`
Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add src/engine.rs tests/golden.rs
git commit -m "feat(engine): FillModel seam + modeled_fills; pin goldens to SameBarClose"
```

---

### Task 6: Engine — deferred no-lookahead fills (`NextBarOpen`) + golden + no-lookahead test

Defer a decision made on bar `t` to fill at `fill_model.fill_bar(t)`, clamped to
the last bar. The pending decision executes at the **top** of the fill bar's
iteration, before `current_pnl` and `on_bar`, so the strategy sees the fill on
the next bar — not the bar it decided on.

**Files:**
- Modify: `src/engine.rs` (`run` loop)
- Test: `tests/golden.rs` (new `golden_options_next_bar_open`, new `next_bar_open_has_no_lookahead`)

**Interfaces:**
- Consumes: `with_fill_model`, `modeled_fills`, `NextBarOpen` (Task 5).
- Produces: no new public API; `run` now defers fills per the model.

- [ ] **Step 1: Write the failing test**

```rust
// tests/golden.rs — add
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
    assert!((pnl - -1244.091014149768).abs() > 1e-6, "expected a different fill than SameBarClose, got {pnl}");
    // CAPTURE-THEN-FREEZE: paste the printed value below and switch to ==.
    eprintln!("NextBarOpen straddle pnl = {pnl}");
}

// A strategy that opens on the LAST bar cannot fill (no t+1) — position stays open.
struct OpenOnLastBar {
    n: usize,
    i: usize,
}
impl Strategy for OpenOnLastBar {
    fn init(&mut self, _p: &HashMap<String, serde_json::Value>) {}
    fn on_bar(&mut self, _ts: i64, _o: f64, _h: f64, _l: f64, c: f64, _v: f64, _pnl: Option<f64>) -> Option<Signal> {
        self.i += 1;
        if self.i == self.n {
            let atm = (c / 100.0).round() * 100.0;
            return Some(Signal {
                action: SignalAction::OpenPosition {
                    legs: vec![OptionLeg {
                        strike: atm, option_type: "CE".into(), entry_price: 0.0,
                        exit_price: None, action: "SELL".into(), expiry_days: Some(7.0),
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
    assert_eq!(trades.len(), 0, "last-bar decision must not fill via lookahead");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p vajra --test golden next_bar_open_has_no_lookahead`
Expected: FAIL — today's engine fills same-bar, so a last-bar open would (wrongly) fill; and `run` doesn't defer yet.

- [ ] **Step 3: Write minimal implementation**

Rewrite the `run` loop to defer fills. Replace the body of the `for i in 0..closes.len()` loop so that: (a) a pending decision due at bar `i` executes first, (b) a new signal on bar `i` schedules a fill at `fill_bar(i)` clamped to `n-1`, executing immediately if that equals `i` (SameBarClose) else stored.

```rust
    pub fn run(&mut self, df: &DataFrame, strategy: &mut dyn Strategy) -> Result<Vec<Trade>> {
        self.modeled_fills = 0;
        let closes = df.column("close")?.f64()?;
        let opens = df.column("open")?.f64()?;
        let highs = df.column("high")?.f64()?;
        let lows = df.column("low")?.f64()?;
        let volumes_series = df.column("volume")?.cast(&DataType::Float64)?;
        let volumes = volumes_series.f64()?;
        let timestamps = df.column("timestamp")?.str()?;
        let ivs = df.column("iv").ok().and_then(|c| c.f64().ok());
        let n = closes.len();

        // A decision awaiting its fill bar (single-position engine => at most one).
        struct Pending {
            signal: Signal,
            fill_bar: usize,
        }
        let mut pending: Option<Pending> = None;

        // Resolve timestamp + underlying fill price at a given bar, per the fill model.
        let bar_ts = |i: usize| -> i64 {
            let dt = chrono::NaiveDateTime::parse_from_str(timestamps.get(i).unwrap(), "%Y-%m-%d %H:%M:%S")
                .unwrap_or_default();
            dt.and_utc().timestamp()
        };

        for i in 0..n {
            let close = closes.get(i).unwrap();
            let open = opens.get(i).unwrap();
            let high = highs.get(i).unwrap();
            let low = lows.get(i).unwrap();
            let volume = volumes.get(i).unwrap();
            let iv = ivs.and_then(|c| c.get(i)).unwrap_or(self.params.iv_assumption);

            // (A) Execute a pending fill scheduled for this bar, before the strategy sees it.
            if pending.as_ref().is_some_and(|p| p.fill_bar == i) {
                let p = pending.take().unwrap();
                let fill_px = self.fill_underlying_px(i, open, high, low, close);
                self.process_signal(p.signal, bar_ts(i), fill_px, iv);
            }

            // (B) Mark unrealized PnL against this bar's close (unchanged behavior).
            let timestamp = bar_ts(i);
            let current_pnl = self.mark_current_pnl(close, timestamp, iv);

            // (C) Ask the strategy; schedule any new decision at its fill bar.
            if let Some(signal) =
                strategy.on_bar(timestamp, open, high, low, close, volume, current_pnl)
            {
                let fb = self.fill_model.fill_bar(i);
                if fb >= n {
                    // ponytail: last-bar decision has no forward bar to fill at; drop it
                    // rather than fill same-bar (which would be lookahead).
                    log::debug!("decision on final bar {i} dropped (no fill bar)");
                } else if fb == i {
                    let fill_px = self.fill_underlying_px(i, open, high, low, close);
                    self.process_signal(signal, timestamp, fill_px, iv);
                } else {
                    pending = Some(Pending { signal, fill_bar: fb });
                }
            }
        }

        Ok(self.trades.clone())
    }
```

Extract two helpers so `run` stays readable — add these methods to `impl BacktestEngine`:

```rust
    fn fill_underlying_px(&self, _i: usize, open: f64, high: f64, low: f64, close: f64) -> f64 {
        match self.fill_model.underlying_field() {
            BarField::Open => open,
            BarField::High => high,
            BarField::Low => low,
            BarField::Close => close,
        }
    }

    fn mark_current_pnl(&self, close: f64, timestamp: i64, iv: f64) -> Option<f64> {
        let trade = self.position.as_ref()?;
        if trade.direction == "SPREAD" {
            let mut current_value = 0.0;
            for leg in &trade.legs {
                let greeks_price = if leg.option_type == "EQ" {
                    close
                } else {
                    let is_call = leg.option_type == "CE";
                    let t = thursday_t(timestamp);
                    let greeks = calculate_greeks(
                        close, leg.strike, t, self.params.risk_free_rate, iv, is_call,
                    );
                    greeks.price
                };
                if leg.action == "SELL" {
                    current_value += greeks_price;
                } else {
                    current_value -= greeks_price;
                }
            }
            Some((trade.entry_price - current_value) * trade.quantity as f64 * self.params.lot_size)
        } else {
            Some((close - trade.entry_price) * trade.quantity as f64)
        }
    }
```

Add the shared time-to-expiry helper (extracted from the three duplicated
Thursday loops) as a free function at the bottom of the file, and replace the
inline Thursday loops in `handle_open_position` and `handle_close_all` with
`let t = thursday_t(timestamp);` (leaving the rest of those functions intact):

```rust
// free function, bottom of src/engine.rs
/// Years to the next Thursday expiry (SP1 keeps the v0.1 calendar; SP3 generalizes).
fn thursday_t(timestamp: i64) -> f64 {
    use chrono::Datelike;
    let dt = chrono::DateTime::from_timestamp(timestamp, 0)
        .unwrap_or_default()
        .naive_utc();
    let mut days = 0.0;
    let mut d = dt.date();
    while d.weekday() != chrono::Weekday::Thu {
        d = d.succ_opt().unwrap();
        days += 1.0;
    }
    if days == 0.0 {
        days = 0.1;
    }
    days / 365.0
}
```

In `handle_open_position`, replace the block from `use chrono::Datelike;` through
`let t = days_to_expiry / 365.0;` with `let t = thursday_t(timestamp);`. Do the
same in `handle_close_all`. Behavior is identical (same math, extracted).

- [ ] **Step 4: Run tests to capture + verify**

Run: `cargo test -p vajra --test golden -- --nocapture`
Expected: `next_bar_open_has_no_lookahead` PASS; `golden_options_next_bar_open` PASS with an `eprintln!` line `NextBarOpen straddle pnl = <X>`. Copy `<X>` into the test, replace the `> 1e-6` inequality with the frozen equality below, keep the SameBarClose goldens green:

```rust
    let pnl = trades[0].pnl.unwrap();
    assert!((pnl - <X>).abs() < 1e-6, "pnl was {pnl}"); // captured, frozen
```

- [ ] **Step 5: Re-run to confirm frozen**

Run: `cargo test -p vajra --test golden && cargo test -p vajra --lib`
Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
git add src/engine.rs tests/golden.rs
git commit -m "feat(engine): deferred no-lookahead fills (NextBarOpen default) + goldens"
```

---

### Task 7: Engine — real-quote marking via `OptionQuoteSource` + `modeled_fills`

Wire an optional option-quote source into the engine. When opening/closing an
option leg, look up a real quote at the fill bar's timestamp; if present, price
from it (via `fill_model.quote_price`) and apply spread-aware slippage; if
absent, use BS and increment `modeled_fills`.

**Files:**
- Modify: `src/engine.rs` (`handle_open_position`, `handle_close_all`, fields/builder)
- Create: `tests/fixtures/opt_long.csv`
- Test: `tests/golden.rs` (`real_quote_marks_from_quote_not_bs`, `modeled_fills_counts_bs_fallback`)

**Interfaces:**
- Consumes: `OptionQuoteSource`, `InstrumentKey`, `Quote`, `MapOptionSource` (Tasks 1-2); `adjust_fill_spread` (Task 4).
- Produces: field `option_source: Option<Box<dyn crate::marketdata::OptionQuoteSource>>` (default `None`); builder `fn with_option_source(mut self, s: Box<dyn crate::marketdata::OptionQuoteSource>) -> Self`. Leg pricing now consults it before BS.

- [ ] **Step 1: Create the fixture**

`tests/fixtures/opt_long.csv` — quotes for the two straddle legs at the ATM
strike, at the two bar timestamps the `FixedStraddle` acts on. Use timestamps
that match rows 5 (open) and 15 (close) of `eq_ohlcv.csv`. Read those two
`timestamp` values and the bar's ATM strike first:

Run: `sed -n '6p;16p' tests/fixtures/eq_ohlcv.csv`
Then write the fixture using those two timestamps `<TS_OPEN>`, `<TS_CLOSE>` and
the ATM strike `<K>` = round(close_at_that_bar/100)*100:

```csv
timestamp,strike,option_type,open,high,low,close,bid,ask
<TS_OPEN>,<K>,CE,120.0,121.0,119.0,120.0,119.5,120.5
<TS_OPEN>,<K>,PE,110.0,111.0,109.0,110.0,109.5,110.5
<TS_CLOSE>,<K>,CE,90.0,91.0,89.0,90.0,89.5,90.5
<TS_CLOSE>,<K>,PE,100.0,101.0,99.0,100.0,99.5,100.5
```

- [ ] **Step 2: Write the failing test**

```rust
// tests/golden.rs — add
use vajra::marketdata::MapOptionSource;

#[test]
fn real_quote_marks_from_quote_not_bs() {
    let df = load("tests/fixtures/eq_ohlcv.csv");
    let opt = load("tests/fixtures/opt_long.csv");
    let source = MapOptionSource::from_long_df(&opt).unwrap();
    let mut strat = FixedStraddle { bar: 0 };
    strat.init(&HashMap::new());
    // Pin SameBarClose so fill bars line up with the fixture timestamps (rows 5 & 15).
    let mut engine = BacktestEngine::new(300_000.0)
        .with_fill_model(Box::new(vajra::fill::SameBarClose))
        .with_option_source(Box::new(source));
    let trades = engine.run(&df, &mut strat).unwrap();
    assert_eq!(trades.len(), 1);
    // All 4 leg fills (2 open + 2 close) came from real quotes => zero modeled.
    assert_eq!(engine.modeled_fills(), 0, "expected all legs marked from real quotes");
    // PnL must differ from the all-BS SameBarClose golden (-1244.091...).
    let pnl = trades[0].pnl.unwrap();
    assert!((pnl - -1244.091014149768).abs() > 1e-6, "pnl should reflect real quotes, got {pnl}");
}

#[test]
fn modeled_fills_counts_bs_fallback() {
    // No option source => every option leg falls back to BS. Straddle = 2 legs
    // on open + 2 on close = 4 modeled fills.
    let df = load("tests/fixtures/eq_ohlcv.csv");
    let mut strat = FixedStraddle { bar: 0 };
    strat.init(&HashMap::new());
    let mut engine = BacktestEngine::new(300_000.0)
        .with_fill_model(Box::new(vajra::fill::SameBarClose));
    let _ = engine.run(&df, &mut strat).unwrap();
    assert_eq!(engine.modeled_fills(), 4);
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p vajra --test golden real_quote_marks_from_quote_not_bs`
Expected: FAIL — `with_option_source` not found.

- [ ] **Step 4: Write minimal implementation**

Add the field to the struct and to **both** `new` and `with_cost` literals:

```rust
    option_source: Option<Box<dyn crate::marketdata::OptionQuoteSource>>,
```
```rust
            option_source: None,
```

Add the builder (after `with_fill_model`):

```rust
    pub fn with_option_source(mut self, s: Box<dyn crate::marketdata::OptionQuoteSource>) -> Self {
        self.option_source = Some(s);
        self
    }
```

Add a private leg-pricing helper that centralizes the real-vs-BS branch. It
needs the fill-bar timestamp; pass it in. Add to `impl BacktestEngine`:

```rust
    /// Price one option leg at the fill bar: real quote if available, else BS
    /// (counted). Returns the pre-slippage price and the quoted spread (if any).
    fn price_leg(
        &mut self,
        strike: f64,
        option_type: &str,
        underlying_px: f64,
        timestamp: i64,
        iv: f64,
    ) -> (f64, Option<f64>) {
        let key = crate::marketdata::InstrumentKey { strike, option_type: option_type.to_string() };
        if let Some(src) = &self.option_source {
            if let Some(q) = src.option_quote(&key, timestamp) {
                return (self.fill_model.quote_price(&q), q.spread());
            }
        }
        // Fallback: Black-Scholes off the underlying.
        let is_call = option_type == "CE";
        let t = thursday_t(timestamp);
        let g = calculate_greeks(underlying_px, strike, t, self.params.risk_free_rate, iv, is_call);
        self.modeled_fills += 1;
        (g.price, None)
    }
```

In `handle_open_position`, replace the per-leg price computation. The current
block computes `entry_price` (BS or `price` for EQ) then calls
`self.cost.adjust_fill(...)`. Replace with:

```rust
                let (mut entry_price, spread) = if signal_leg.option_type == "EQ" {
                    (price, None)
                } else {
                    self.price_leg(signal_leg.strike, &signal_leg.option_type, price, timestamp, iv)
                };
                entry_price = self.cost.adjust_fill_spread(
                    entry_price,
                    &signal_leg.action,
                    signal_leg.option_type != "EQ",
                    false,
                    spread,
                );
```

In `handle_close_all`, replace the per-leg exit price computation similarly. The
current block computes `exit_price` (BS or `price` for EQ) then applies exit
slippage. Replace the price computation and the option-branch slippage with the
spread-aware call, **preserving the EQ double-slippage bug exactly**:

```rust
                    let (mut exit_price, spread) = if leg.option_type == "EQ" {
                        (price, None)
                    } else {
                        self.price_leg(leg.strike, &leg.option_type, price, timestamp, iv)
                    };

                    // Exit Slippage
                    if leg.option_type == "EQ" {
                        // vajra: preserves double-slippage bug from source; fix post-extraction
                        exit_price = self.cost.adjust_fill(exit_price, &leg.action, false, true);
                        exit_price = self.cost.adjust_fill(exit_price, &leg.action, false, true);
                    } else {
                        exit_price = self.cost.adjust_fill_spread(exit_price, &leg.action, true, true, spread);
                    }
```

Note: `price_leg` takes `&mut self` (it bumps `modeled_fills`), while
`handle_close_all` iterates `&mut trade.legs`. Resolve the borrow by computing
prices before the mutation: read `leg.strike` / `leg.option_type` (both `Copy`
/ cheap clone) into locals before calling `price_leg`, or restructure the loop
to collect `(exit_price, spread)` first. Simplest: capture `let strike = leg.strike; let otype = leg.option_type.clone();` at the top of the loop body and pass those.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p vajra --test golden`
Expected: PASS — `real_quote_marks_from_quote_not_bs`, `modeled_fills_counts_bs_fallback`, plus all prior goldens still green.

- [ ] **Step 6: Full gate**

Run: `cargo test -p vajra && cargo test -p vajra --no-default-features && cargo clippy -p vajra --all-targets -- -D warnings && cargo fmt --check`
Expected: all green/clean.

- [ ] **Step 7: Commit**

```bash
git add src/engine.rs tests/golden.rs tests/fixtures/opt_long.csv
git commit -m "feat(engine): real-quote marking via OptionQuoteSource + modeled_fills counter"
```

---

### Task 8: Docs — README + CHANGELOG

**Files:**
- Modify: `README.md`
- Modify: `CHANGELOG.md`

**Interfaces:** none (docs only).

- [ ] **Step 1: Update README**

Add a section after "Implied volatility" documenting: (a) the default fill model
is now `NextBarOpen` (lookahead-free); `SameBarClose` is opt-in via
`with_fill_model` for exact pre-0.x reproduction; (b) real option quotes via
`with_option_source(MapOptionSource::from_long_df(&opt_df))`, with the
long-format columns (`timestamp,strike,option_type,open,high,low,close[,bid,ask]`);
(c) `engine.modeled_fills()` reports how many legs used the BS fallback.

```markdown
### Execution & real-quote marking

By default the engine fills **lookahead-free**: a decision on bar *t* fills at
*t+1*'s open (`NextBarOpen`). For bit-identical reproduction of pre-0.x runs,
opt into the old same-bar-close fill:

    let engine = BacktestEngine::new(capital)
        .with_fill_model(Box::new(vajra::fill::SameBarClose));

Option legs are marked from **real quotes** when you supply them, and only fall
back to Black-Scholes otherwise:

    let opt = CsvDataLoader::new().from_path("option_quotes.csv")?; // long format
    let source = vajra::marketdata::MapOptionSource::from_long_df(&opt)?;
    let engine = BacktestEngine::new(capital).with_option_source(Box::new(source));

Long-format columns: `timestamp,strike,option_type,open,high,low,close` plus
optional `bid,ask` (spread widens slippage). After a run,
`engine.modeled_fills()` returns how many option legs used the BS fallback — a
backtest fed complete quotes reports `0`.
```

- [ ] **Step 2: Update CHANGELOG**

Add under an `## [Unreleased]` heading:

```markdown
## [Unreleased]
### Changed
- **Default fill is now lookahead-free** (`NextBarOpen`, t+1 open). Opt into the
  old same-bar-close behavior with `BacktestEngine::with_fill_model(Box::new(SameBarClose))`.
### Added
- `FillModel` trait with `NextBarOpen` / `SameBarClose`.
- Real per-leg option marking via `OptionQuoteSource` / `MapOptionSource`;
  Black-Scholes is now an explicit, counted fallback (`BacktestEngine::modeled_fills()`).
- Spread-aware slippage (`CostModel::adjust_fill_spread`) using quoted bid/ask.
```

- [ ] **Step 3: Commit**

```bash
git add README.md CHANGELOG.md
git commit -m "docs: SP1 execution realism — fill models, real quotes, modeled_fills"
```

- [ ] **Step 4: Push**

```bash
git push
```

---

## Self-Review

**Spec coverage** (SP1 spec → task):
- Real per-leg price series in → Tasks 1,2,7. ✅
- Lookahead-free `FillModel` trait → Tasks 3,6. ✅
- Spread-aware slippage from bid/ask → Tasks 4,7. ✅
- BS as explicit labeled fallback + `modeled_fills` surfaced → Task 7. ✅
- Exact-reproduction path (`SameBarClose` pins goldens) → Task 5. ✅
- New goldens for `NextBarOpen` + real-quote marking → Tasks 6,7. ✅
- Backward-compat tests (no-lookahead, counter, spread monotonicity) → Tasks 4,6,7. ✅
- `--no-default-features` + clippy clean → Tasks 5,7. ✅
- Deferred (documented): Vwap/WorstOfBar, minimal expiry seam → SP3. Flagged above.

**Type consistency:** `Quote`/`InstrumentKey`/`OptionQuoteSource` (Task 1) used verbatim in Tasks 2,7. `FillModel::{fill_bar,underlying_field,quote_price}` + `BarField` (Task 3) used in Tasks 5,6. `adjust_fill_spread` signature (Task 4) matches the call sites in Task 7. `MapOptionSource::from_long_df` (Task 2) matches Task 7 usage. Engine builders `with_fill_model`/`with_option_source` and `modeled_fills()` consistent across Tasks 5-7.

**Placeholder scan:** `<X>`, `<TS_OPEN>`, `<TS_CLOSE>`, `<K>` are capture-then-freeze values the implementer reads from a real run/fixture in-task (Task 6 Step 4, Task 7 Step 1) — not open TODOs.
