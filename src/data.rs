//! CSV data loading for the backtester.

use anyhow::Result;
use polars::prelude::*;

/// Loads OHLCV bar data from CSV files at arbitrary paths.
#[derive(Default)]
pub struct CsvDataLoader;

impl CsvDataLoader {
    pub fn new() -> Self {
        Self
    }

    /// Load a CSV file (with header) into a DataFrame.
    pub fn from_path(&self, path: &str) -> Result<DataFrame> {
        let lf = LazyCsvReader::new(path).has_header(true).finish()?;
        Ok(lf.collect()?)
    }
}
