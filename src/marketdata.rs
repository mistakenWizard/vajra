//! Market-data types and the option-quote overlay used for real-quote marking.

use std::collections::HashMap;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_spread_present_and_absent() {
        let q = Quote {
            open: 10.0,
            high: 11.0,
            low: 9.0,
            close: 10.5,
            bid: Some(10.0),
            ask: Some(10.4),
        };
        assert!((q.spread().unwrap() - 0.4).abs() < 1e-9);
        let q2 = Quote {
            bid: None,
            ask: None,
            ..q
        };
        assert!(q2.spread().is_none());
    }

    #[test]
    fn instrument_key_hashes_by_value() {
        use std::collections::HashMap;
        let mut m = HashMap::new();
        m.insert(
            InstrumentKey {
                strike: 22100.0,
                option_type: "CE".into(),
            },
            1,
        );
        assert_eq!(
            m.get(&InstrumentKey {
                strike: 22100.0,
                option_type: "CE".into()
            }),
            Some(&1)
        );
        assert_eq!(
            m.get(&InstrumentKey {
                strike: 22200.0,
                option_type: "CE".into()
            }),
            None
        );
    }

    #[test]
    fn map_source_returns_inserted_quote() {
        let mut s = MapOptionSource::new();
        let key = InstrumentKey {
            strike: 22100.0,
            option_type: "CE".into(),
        };
        let q = Quote {
            open: 5.0,
            high: 6.0,
            low: 4.0,
            close: 5.5,
            bid: None,
            ask: None,
        };
        s.insert(1000, key.clone(), q);
        assert_eq!(s.option_quote(&key, 1000), Some(q));
        assert_eq!(s.option_quote(&key, 2000), None); // wrong ts
        let pe = InstrumentKey {
            strike: 22100.0,
            option_type: "PE".into(),
        };
        assert_eq!(s.option_quote(&pe, 1000), None); // wrong type
    }
}
