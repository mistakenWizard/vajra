# Contributing to Vajra

Thanks for your interest. Vajra is early (v0.1, API unstable) — bug reports,
test cases, and small focused PRs are all welcome.

## Development

```bash
cargo test                      # full suite
cargo test --no-default-features  # must also stay green
cargo clippy --all-targets -- -D warnings
cargo fmt --all
```

CI runs all four of the above on every push and PR; a PR that fails any of them
won't merge. Run them locally first.

## Golden tests

`tests/golden.rs` pins the P&L of two reference backtests (an equity MA
crossover and a fixed options straddle) to exact float values. These are
**characterization tests**: they lock current behavior so refactors can't
silently change results.

- If your change is a pure refactor, the golden values must **not** move. If
  they do, your change altered behavior — investigate before touching the
  constants.
- If your change *intentionally* changes pricing/cost/fill logic, update the
  golden values in the same PR and explain why in the description.

## Scope

Vajra is a lean, options-native backtesting core. Good contributions: better
fill/cost models, more reference strategies, richer reporting, realistic
market data handling. Please keep the default build runtime-free (no
`tokio`/`reqwest` in `[dependencies]`) — gate any async data fetchers behind an
optional feature.

## PRs

Keep them small and focused. One logical change per PR. Include a test that
fails without your change.

## License

By contributing you agree your work is dual-licensed under Apache-2.0 OR MIT,
matching the project.
