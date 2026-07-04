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
}
