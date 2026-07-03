use vajra::data::CsvDataLoader;

#[test]
fn loads_csv_from_arbitrary_path() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("d.csv");
    std::fs::write(
        &p,
        "timestamp,open,high,low,close,volume\n1,10,11,9,10,100\n",
    )
    .unwrap();
    let df = CsvDataLoader::new().from_path(p.to_str().unwrap()).unwrap();
    assert_eq!(df.height(), 1);
}

#[test]
fn missing_file_errors() {
    assert!(CsvDataLoader::new().from_path("/no/such/file.csv").is_err());
}
