//! Native Rust Greeks Engine
//!
//! Provides Black-Scholes calculations for option pricing and greeks.
//! Used by both the backtester and the autonomous trading module.

use std::f64::consts::PI;

#[derive(Debug, Clone, Copy)]
pub struct OptionGreeks {
    pub price: f64,
    pub delta: f64,
    pub gamma: f64,
    pub vega: f64,
    pub theta: f64,
}

/// Cumulative Distribution Function for the Standard Normal Distribution
/// Uses a high-precision approximation (Hart's Algorithm)
fn nd_cdf(x: f64) -> f64 {
    let a1 = 0.254829592;
    let a2 = -0.284496736;
    let a3 = 1.421413741;
    let a4 = -1.453152027;
    let a5 = 1.061405429;
    let p = 0.3275911;

    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    // A&S 7.1.26 approximates erf(z); the normal CDF needs erf(x/sqrt(2)),
    // so scale the input before the approximation.
    let abs_x = x.abs() / std::f64::consts::SQRT_2;

    let t = 1.0 / (1.0 + p * abs_x);
    let y = 1.0 - (((((a5 * t + a4) * t) + a3) * t + a2) * t + a1) * t * (-abs_x * abs_x).exp();

    0.5 * (1.0 + sign * y)
}

/// Probability Density Function for the Standard Normal Distribution
fn nd_pdf(x: f64) -> f64 {
    (1.0 / (2.0 * PI).sqrt()) * (-0.5 * x * x).exp()
}

/// Calculate Black-Scholes Price and Greeks
///
/// # Arguments
/// * `s` - Current stock price
/// * `k` - Strike price
/// * `t` - Time to expiration (in years)
/// * `r` - Risk-free interest rate (e.g., 0.07 for 7%)
/// * `sigma` - Volatility (standard deviation of returns)
/// * `is_call` - true for Call, false for Put
pub fn calculate_greeks(s: f64, k: f64, t: f64, r: f64, sigma: f64, is_call: bool) -> OptionGreeks {
    if t <= 0.0 {
        let price = if is_call {
            (s - k).max(0.0)
        } else {
            (k - s).max(0.0)
        };
        return OptionGreeks {
            price,
            delta: if is_call {
                if s > k {
                    1.0
                } else {
                    0.0
                }
            } else {
                if s < k {
                    -1.0
                } else {
                    0.0
                }
            },
            gamma: 0.0,
            vega: 0.0,
            theta: 0.0,
        };
    }

    let d1 = ((s / k).ln() + (r + 0.5 * sigma * sigma) * t) / (sigma * t.sqrt());
    let d2 = d1 - sigma * t.sqrt();

    let (price, delta, theta) = if is_call {
        let p = s * nd_cdf(d1) - k * (-r * t).exp() * nd_cdf(d2);
        let d = nd_cdf(d1);
        let th =
            -((s * nd_pdf(d1) * sigma) / (2.0 * t.sqrt())) - r * k * (-r * t).exp() * nd_cdf(d2);
        (p, d, th)
    } else {
        let p = k * (-r * t).exp() * nd_cdf(-d2) - s * nd_cdf(-d1);
        let d = nd_cdf(d1) - 1.0;
        let th =
            -((s * nd_pdf(d1) * sigma) / (2.0 * t.sqrt())) + r * k * (-r * t).exp() * nd_cdf(-d2);
        (p, d, th)
    };

    let gamma = nd_pdf(d1) / (s * sigma * t.sqrt());
    let vega = s * t.sqrt() * nd_pdf(d1);

    OptionGreeks {
        price,
        delta,
        gamma,
        vega,
        theta,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_black_scholes_call() {
        // Example: S=100, K=100, T=1 (year), R=0.05, Sigma=0.2
        let greeks = calculate_greeks(100.0, 100.0, 1.0, 0.05, 0.2, true);

        // Expected price is approx 10.45
        assert!((greeks.price - 10.45).abs() < 0.01);
        // Expected delta is approx 0.637
        assert!((greeks.delta - 0.637).abs() < 0.01);
    }

    #[test]
    fn test_black_scholes_put() {
        let greeks = calculate_greeks(100.0, 100.0, 1.0, 0.05, 0.2, false);

        // Expected price is approx 5.57
        assert!((greeks.price - 5.57).abs() < 0.01);
        // Expected delta is approx -0.363
        assert!((greeks.delta - (-0.363)).abs() < 0.01);
    }
}
