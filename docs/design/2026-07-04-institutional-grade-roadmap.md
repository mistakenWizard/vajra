# Vajra → Institutional-Grade: Roadmap & Sub-Project 1 Spec

Status: **DRAFT for review** · 2026-07-04

## What "institutional-grade" means here (the bar)

A backtester whose **option P&L you can stake real capital on**. Concretely:

1. Option legs are marked to **real quotes when available**, not synthesized from a
   flat vol guess.
2. Execution is **realistic and lookahead-free**: a decision on bar *t* fills at
   *t+1*, at a price with modeled slippage — never at a price the strategy couldn't
   have gotten.
3. Costs, margin, and expiry/roll mechanics reflect the **actual venue**, supplied
   as data, not hardcoded to one index.
4. The pricing/greeks core is **validated against a reference** and guarded by
   property + no-lookahead tests.

Secondary (later): portfolio breadth, risk analytics, OSS 1.0 polish. Primary
driver for this effort = **trustworthy P&L**, with correctness/rigor woven in.

## Honest baseline (vajra v0.1, ~1,100 LOC)

| Area | Today | Institutional gap |
|------|-------|-------------------|
| Option pricing | BS synthesized from underlying `close` + flat/per-bar IV | Can't consume real CE/PE candles; every option fill is a model guess |
| Expiry | hardcoded "next Thursday" loop (`engine.rs:285`) | One index's calendar baked into the pricing time-to-expiry |
| Fills | same-bar `close` + flat ±0.5 slippage | Lookahead (decides and fills on the same bar); slippage ignores spread/liquidity |
| Positions | single position, single symbol, single strategy per run | No portfolio, no concurrent legs across underlyings |
| Margin | scalar `margin_multiplier` + hardcoded ₹150k SEBI floor | No real SPAN/exposure margin |
| Market config | `"NIFTY"` symbol literal (`:366`), lot 50 default | Not parameterized per instrument |
| Validation | 2 golden tests + 2 greeks unit tests | No reference-pricer check, no property tests, no no-lookahead guarantee, no benchmarks |
| Costs | `CostModel` trait ✅ (ZeroCost / IndiaOptionsCost) | Good foundation — keep |

Strengths to preserve: clean `Strategy` / `CostModel` traits, corrected BS greeks,
runtime-free core, golden-test discipline, per-bar IV column.

## Decomposition — 6 sub-projects, in build order

Each is its own spec → plan → implementation cycle. Ordered so the highest P&L-trust
payoff lands first and later work builds on stable seams.

1. **SP1 — Execution realism & real-quote marking** *(this spec)*
   Real option-price series in; `FillModel` trait (no-lookahead, t+1 default);
   spread-aware slippage. The credibility core.
2. **SP2 — Correctness harness**
   Validate greeks vs QuantLib (test-only), property tests (put-call parity,
   monotonicity, delta bounds), a structural no-lookahead test, expanded goldens,
   `criterion` benchmarks.
3. **SP3 — Market config as data**
   Replace the Thursday/`NIFTY`/lot hardcodes with an `InstrumentSpec` /
   `ExpiryCalendar` supplied by the caller. Unblocks non-NIFTY and non-Indian use.
4. **SP4 — Portfolio & margin engine**
   Multiple concurrent positions, multi-symbol runs, a pluggable `MarginModel`
   (SPAN-lite), position-sizing hooks.
5. **SP5 — Risk analytics & tearsheet**
   Portfolio greeks over time, drawdown decomposition, VaR/CVaR, per-leg
   attribution, richer report.
6. **SP6 — OSS hardening → 1.0**
   API stabilization + semver, docs.rs coverage, `crates.io` publish, CI matrix +
   MSRV.

Dependency notes: SP2 can run in parallel with SP1 (it tests the existing core).
SP3 is a soft prerequisite for SP1's pricing fallback being honest, so SP1 folds in
the *minimal* expiry-input change it needs and SP3 generalizes the rest. SP4/SP5
depend on SP1's execution seams.

---

## Sub-Project 1 — Execution realism & real-quote marking

### Problem
An options backtester that invents option prices from a flat vol, then fills them on
the same bar the signal fired, produces P&L that is *both* mispriced and
lookahead-biased. The user's own data is real 1-minute option candles (CE/PE OHLC) —
vajra must consume them directly and only fall back to model pricing when a quote is
genuinely absent.

### Goals
- Mark and fill option legs from **real per-leg price series** when supplied.
- Make fills **lookahead-free** and configurable via a `FillModel` trait.
- Make slippage a function of **quoted spread** when bid/ask are available.
- Keep BS pricing as an explicit, labeled **fallback**, never the silent default.
- Preserve an **exact-reproduction path**: callers who opt into `SameBarClose` get
  bit-identical numbers to v0.1. The *new default* (`NextBarOpen`) intentionally
  changes results by removing lookahead — a documented 0.x behavior change, not a
  silent one. Existing golden fixtures are pinned under `SameBarClose`; new goldens
  cover `NextBarOpen` and real-quote marking.

### Non-goals (deferred)
Portfolio/multi-position (SP4), full market-config generalization (SP3), margin
engine (SP4), reference-pricer validation (SP2).

### Design

**1. Instrument-keyed price access.** Introduce a small `PriceSource` seam the engine
queries instead of reaching into a single `close` column:

```rust
/// Resolves the tradeable price of an instrument at a bar index.
pub trait PriceSource {
    /// OHLC for the underlying at bar `i`.
    fn underlying(&self, i: usize) -> Ohlc;
    /// Real quote for an option leg at bar `i`, if a series exists for it.
    /// `None` ⇒ engine falls back to model pricing (logged).
    fn option_quote(&self, key: &InstrumentKey, i: usize) -> Option<Quote>;
    fn len(&self) -> usize;
}
```

- `InstrumentKey` = `{ strike, option_type, expiry }` (a light newtype, not a broker id).
- `Quote` carries `{ open, high, low, close, bid: Option<f64>, ask: Option<f64> }`.
- Default impl `DataFramePriceSource` reads the existing single-DataFrame layout for
  the underlying and, if present, columns/side-tables named by convention
  (e.g. `ce_<strike>_close`, or a tidy long-format option frame). Absent ⇒ `None`.

**2. `FillModel` trait — the no-lookahead seam.**

```rust
pub trait FillModel {
    /// Given a decision on bar `decision_i`, return the (bar_index, price) the
    /// order actually fills at. Default: next bar's open.
    fn resolve(&self, decision_i: usize, side: Side, quote: &Quote) -> Fill;
}
```

- Built-ins: `NextBarOpen` (**default**), `SameBarClose` (explicit opt-in — the old
  behavior, so goldens can pin it), `Vwap`, `WorstOfBar`.
- The engine's entry/exit paths call `FillModel::resolve` instead of grabbing `close`
  directly. Slippage is then applied by the existing `CostModel::adjust_fill`,
  extended to accept an optional spread so `IndiaOptionsCost` can widen the fill by a
  fraction of `(ask − bid)` when present, flat otherwise.

**3. Pricing fallback, made explicit.** When `option_quote` returns `None`, the engine
prices via BS exactly as today, but emits `log::debug!("no quote for {key}; modeling
at iv={}")` and increments a `modeled_fills` counter surfaced in the run result — so a
backtest reports *how much* of its P&L was synthesized vs real.

**4. Minimal expiry input (SP3 seam).** `InstrumentKey.expiry` is supplied by the
signal/leg rather than derived from the Thursday loop. The loop stays only as the
fallback default when a leg omits expiry, keeping current callers working. Full
calendar generalization is SP3.

### Data flow

```
Strategy.on_bar → Signal(legs)
   → engine, at each leg:
       key = InstrumentKey{strike, type, expiry}
       fill = FillModel.resolve(decision_i, side, quote_or_modeled)
       price = quote.close (real)  OR  BS(underlying, iv, t)  [modeled++]
       price = CostModel.adjust_fill(price, side, is_option, spread)
   → Trade recorded with real-vs-modeled provenance
```

### Backward compatibility & tests
- No `FillModel` passed ⇒ engine uses `SameBarClose` internally? **No** — default is
  `NextBarOpen`, which *would* move goldens. Resolution: keep the current golden
  fixtures pinned under an explicit `SameBarClose` config in `golden.rs`, and add
  **new** goldens for `NextBarOpen` + real-quote marking. This makes the lookahead
  fix visible and measured rather than hidden.
- New tests: (a) real-quote marking uses the quote, not BS, when a series is present;
  (b) `NextBarOpen` fills at `t+1` open and a strategy cannot peek at `t`'s close for
  its own fill; (c) `modeled_fills` counter accounts correctly on mixed data;
  (d) spread-aware slippage widens fills monotonically with `(ask−bid)`.
- `--no-default-features` stays green; clippy `-D warnings` clean.

### Success criteria
- A backtest fed real CE/PE candles prices **0 legs** via the BS fallback
  (`modeled_fills == 0`) and its P&L matches a hand-checked manual replay.
- Flipping `SameBarClose → NextBarOpen` changes results in the expected direction and
  the delta is attributable to the removed lookahead.
- Existing (underlying-only) callers get identical numbers under `SameBarClose`.

### Risks
- **Data-shape sprawl**: option series can be wide (`ce_<strike>_close`) or long
  (tidy). Pick **one** canonical long format for the loader; document it; provide a
  helper to pivot. (ponytail: one format, add the other when a real dataset needs it.)
- **Golden churn**: mitigated by the explicit `SameBarClose` pin above.
