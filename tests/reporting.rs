use vajra::engine::Trade;
use vajra::reporting::write_trades_csv;

fn sample() -> Vec<Trade> {
    vec![Trade {
        symbol: "X".into(),
        entry_time: 1,
        exit_time: Some(2),
        entry_price: 100.0,
        exit_price: Some(90.0),
        quantity: 1,
        pnl: Some(10.0),
        direction: "SHORT".into(),
        legs: vec![],
    }]
}

#[test]
fn writes_trades_csv_with_header_and_row() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("t.csv");
    write_trades_csv(&sample(), p.to_str().unwrap()).unwrap();
    let out = std::fs::read_to_string(&p).unwrap();
    assert!(
        out.contains("symbol,entry_time,exit_time,entry_price,exit_price,quantity,pnl,direction")
    );
    assert!(out.contains("X,1,2,100"));
}
