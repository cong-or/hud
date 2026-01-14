use hud::export::TraceEventExporter;
use hud::symbolization::Symbolizer;

#[test]
fn test_export_creates_valid_json() {
    // Create a symbolizer (we can use any binary for testing)
    let binary_path = env!("CARGO_BIN_EXE_hud");
    let symbolizer = Symbolizer::new(binary_path).expect("Failed to create symbolizer");

    // Create an exporter and export to an in-memory buffer
    let exporter = TraceEventExporter::new(symbolizer);
    let mut buffer = Vec::new();

    exporter.export(&mut buffer).expect("Failed to export trace");

    // Verify the output is valid JSON
    let json_str = String::from_utf8(buffer).expect("Invalid UTF-8");
    let parsed: serde_json::Value = serde_json::from_str(&json_str).expect("Invalid JSON");

    // Verify it has the expected structure
    assert!(parsed.get("traceEvents").is_some());
    assert!(parsed.get("displayTimeUnit").is_some());
    assert_eq!(parsed["displayTimeUnit"], "ms");
}
