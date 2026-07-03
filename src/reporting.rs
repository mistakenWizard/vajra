use crate::engine::Trade;
use anyhow::Result;
use chrono::{DateTime, Utc};
use std::fs;

pub struct PerformanceMetrics {
    pub total_trades: usize,
    pub total_pnl: f64,
    pub win_rate: f64,
    pub max_drawdown: f64,
    pub annualized_return: f64,
    pub sharpe_ratio: f64,
    pub calmar_ratio: f64,
}

pub fn generate_report(trades: &[Trade], initial_capital: f64, output_path: &str) -> Result<()> {
    let metrics = calculate_metrics(trades, initial_capital);
    let html = generate_html(trades, &metrics, initial_capital);
    fs::write(output_path, html)?;
    Ok(())
}

fn calculate_metrics(trades: &[Trade], initial_capital: f64) -> PerformanceMetrics {
    let total_trades = trades.len();
    if total_trades == 0 {
        return PerformanceMetrics {
            total_trades: 0,
            total_pnl: 0.0,
            win_rate: 0.0,
            max_drawdown: 0.0,
            annualized_return: 0.0,
            sharpe_ratio: 0.0,
            calmar_ratio: 0.0,
        };
    }

    let mut total_pnl = 0.0;
    let mut wins = 0;
    let mut peak_equity = initial_capital;
    let mut current_equity = initial_capital;
    let mut max_drawdown = 0.0;

    // For Sharpe Ratio
    let mut daily_returns: Vec<f64> = Vec::new();
    let mut last_equity = initial_capital;

    // Determine duration for annualization
    let first_time = trades.first().map(|t| t.entry_time).unwrap_or(0);
    let last_time = trades
        .last()
        .and_then(|t| t.exit_time)
        .unwrap_or(first_time + 86400);
    let duration_days = (last_time - first_time) as f64 / 86400.0;
    let years = (duration_days / 365.25).max(0.1);

    for trade in trades {
        if let Some(pnl) = trade.pnl {
            total_pnl += pnl;
            if pnl > 0.0 {
                wins += 1;
            }
            current_equity += pnl;

            // Simple daily return proxy (per trade)
            let ret = pnl / last_equity;
            daily_returns.push(ret);
            last_equity = current_equity;

            if current_equity > peak_equity {
                peak_equity = current_equity;
            }
            let drawdown = (peak_equity - current_equity) / peak_equity * 100.0;
            if drawdown > max_drawdown {
                max_drawdown = drawdown;
            }
        }
    }

    let annualized_return = ((current_equity / initial_capital).powf(1.0 / years) - 1.0) * 100.0;

    // Sharpe Calculation (Simplified: assumes 252 trading days for volatility scaling)
    let avg_ret = daily_returns.iter().sum::<f64>() / daily_returns.len() as f64;
    let variance = daily_returns
        .iter()
        .map(|r| (r - avg_ret).powi(2))
        .sum::<f64>()
        / daily_returns.len() as f64;
    let std_dev = variance.sqrt();
    let sharpe_ratio = if std_dev > 0.0 {
        (avg_ret / std_dev) * (252.0_f64).sqrt()
    } else {
        0.0
    };

    let calmar_ratio = if max_drawdown > 0.0 {
        annualized_return / max_drawdown
    } else {
        0.0
    };

    PerformanceMetrics {
        total_trades,
        total_pnl,
        win_rate: (wins as f64 / total_trades as f64) * 100.0,
        max_drawdown,
        annualized_return,
        sharpe_ratio,
        calmar_ratio,
    }
}

fn generate_html(trades: &[Trade], metrics: &PerformanceMetrics, initial_capital: f64) -> String {
    let mut equity_curve = vec![initial_capital];
    let mut current = initial_capital;
    let mut labels = vec!["Start".to_string()];

    for trade in trades.iter() {
        if let Some(pnl) = trade.pnl {
            current += pnl;
            equity_curve.push(current);
            let dt = DateTime::<Utc>::from_timestamp(trade.entry_time, 0).unwrap_or_default();
            labels.push(dt.format("%Y-%m-%d").to_string());
        }
    }

    let equity_data = serde_json::to_string(&equity_curve).unwrap();
    let labels_data = serde_json::to_string(&labels).unwrap();

    let table_rows: String = trades
        .iter()
        .map(|t| {
            let dt = DateTime::<Utc>::from_timestamp(t.entry_time, 0).unwrap_or_default();
            format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{:.2}</td><td>{:.2}</td><td class='{}'>{:.2}</td></tr>",
                dt.format("%Y-%m-%d %H:%M"),
                t.symbol,
                t.direction,
                t.entry_price,
                t.exit_price.unwrap_or(0.0),
                if t.pnl.unwrap_or(0.0) >= 0.0 { "pos" } else { "neg" },
                t.pnl.unwrap_or(0.0)
            )
        })
        .collect();

    format!(
        r#"
<!DOCTYPE html>
<html>
<head>
    <title>Institutional Backtest Report</title>
    <script src="https://cdn.jsdelivr.net/npm/chart.js"></script>
    <style>
        body {{ font-family: 'Segoe UI', Tahoma, Geneva, Verdana, sans-serif; margin: 40px; background-color: #f8f9fa; color: #333; }}
        h1, h2 {{ color: #2c3e50; border-bottom: 2px solid #3498db; padding-bottom: 10px; }}
        .stats {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 20px; margin-bottom: 30px; }}
        .card {{ background: white; padding: 20px; border-radius: 8px; box-shadow: 0 4px 6px rgba(0,0,0,0.1); text-align: center; }}
        .card h3 {{ margin: 0; color: #7f8c8d; font-size: 0.9em; text-transform: uppercase; }}
        .card p {{ margin: 10px 0 0 0; font-size: 1.8em; font-weight: bold; color: #2980b9; }}
        .chart-container {{ background: white; padding: 20px; border-radius: 8px; box-shadow: 0 4px 6px rgba(0,0,0,0.1); margin-bottom: 30px; }}
        table {{ width: 100%; border-collapse: collapse; background: white; border-radius: 8px; overflow: hidden; box-shadow: 0 4px 6px rgba(0,0,0,0.1); }}
        th, td {{ padding: 12px 15px; text-align: left; border-bottom: 1px solid #eee; }}
        th {{ background-color: #3498db; color: white; text-transform: uppercase; font-size: 0.85em; }}
        tr:hover {{ background-color: #f1f1f1; }}
        .pos {{ color: #27ae60; }}
        .neg {{ color: #c0392b; }}
    </style>
</head>
<body>
    <h1>Institutional Backtest Summary</h1>
    
    <div class="stats">
        <div class="card"><h3>Net PnL</h3><p>₹{:.2}</p></div>
        <div class="card"><h3>Annualized Return</h3><p>{:.2}%</p></div>
        <div class="card"><h3>Sharpe Ratio</h3><p>{:.2}</p></div>
        <div class="card"><h3>Calmar Ratio</h3><p>{:.2}</p></div>
        <div class="card"><h3>Max Drawdown</h3><p class="neg">{:.2}%</p></div>
        <div class="card"><h3>Win Rate</h3><p>{:.2}%</p></div>
        <div class="card"><h3>Total Trades</h3><p>{}</p></div>
    </div>

    <div class="chart-container">
        <h2>Equity Growth</h2>
        <canvas id="equityChart"></canvas>
    </div>

    <h2>Detailed Trade Log</h2>
    <table>
        <thead>
            <tr><th>Date</th><th>Symbol</th><th>Type</th><th>Entry</th><th>Exit</th><th>PnL</th></tr>
        </thead>
        <tbody>{}</tbody>
    </table>

    <script>
        const ctx = document.getElementById('equityChart').getContext('2d');
        new Chart(ctx, {{
            type: 'line',
            data: {{
                labels: {},
                datasets: [{{
                    label: 'Portfolio Value (₹)',
                    data: {},
                    borderColor: '#3498db',
                    backgroundColor: 'rgba(52, 152, 219, 0.1)',
                    fill: true,
                    tension: 0.3,
                    pointRadius: 2
                }}]
            }},
            options: {{
                responsive: true,
                plugins: {{ legend: {{ display: false }} }},
                scales: {{
                    y: {{ beginAtZero: false, grid: {{ color: '#f0f0f0' }} }},
                    x: {{ grid: {{ display: false }} }}
                }}
            }}
        }});
    </script>
</body>
</html>
"#,
        metrics.total_pnl,
        metrics.annualized_return,
        metrics.sharpe_ratio,
        metrics.calmar_ratio,
        metrics.max_drawdown,
        metrics.win_rate,
        metrics.total_trades,
        table_rows,
        labels_data,
        equity_data
    )
}

/// Write trades to a machine-readable CSV (header + one row per trade).
pub fn write_trades_csv(trades: &[Trade], path: &str) -> Result<()> {
    let mut w = csv::Writer::from_path(path)?;
    w.write_record([
        "symbol",
        "entry_time",
        "exit_time",
        "entry_price",
        "exit_price",
        "quantity",
        "pnl",
        "direction",
    ])?;
    for t in trades {
        w.write_record([
            &t.symbol,
            &t.entry_time.to_string(),
            &t.exit_time.map(|v| v.to_string()).unwrap_or_default(),
            &t.entry_price.to_string(),
            &t.exit_price.map(|v| v.to_string()).unwrap_or_default(),
            &t.quantity.to_string(),
            &t.pnl.map(|v| v.to_string()).unwrap_or_default(),
            &t.direction,
        ])?;
    }
    w.flush()?;
    Ok(())
}
