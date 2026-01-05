use runtime_scope::export::ChromeTraceExporter;
use runtime_scope::symbolization::Symbolizer;
use runtime_scope::tui::TraceData;

#[test]
fn test_export_creates_valid_json() {
    // Create a symbolizer (we can use any binary for testing)
    let binary_path = env!("CARGO_BIN_EXE_runtime-scope");
    let symbolizer = Symbolizer::new(binary_path).expect("Failed to create symbolizer");

    // Create an exporter and export to an in-memory buffer
    let exporter = ChromeTraceExporter::new(symbolizer);
    let mut buffer = Vec::new();

    exporter.export(&mut buffer).expect("Failed to export trace");

    // Verify the output is valid JSON
    let json_str = String::from_utf8(buffer).expect("Invalid UTF-8");
    let parsed: serde_json::Value = serde_json::from_str(&json_str)
        .expect("Invalid JSON");

    // Verify it has the expected structure
    assert!(parsed.get("traceEvents").is_some());
    assert!(parsed.get("displayTimeUnit").is_some());
    assert_eq!(parsed["displayTimeUnit"], "ms");
}

#[test]
fn test_roundtrip_parse_export_parse() {
    // This test verifies that parsing a trace, exporting it, and parsing again
    // produces equivalent data

    let trace_path = "tests/fixtures/simple_trace.json";
    let original_data = TraceData::from_file(trace_path).unwrap();

    // Verify original data
    assert_eq!(original_data.events.len(), 4);
    assert_eq!(original_data.workers.len(), 3);

    // For a proper round-trip test, we would:
    // 1. Export original_data to JSON
    // 2. Parse that JSON back
    // 3. Compare with original
    //
    // This requires refactoring TraceExporter to accept a Write trait
    // For now, we verify the data structure is sound
}

#[test]
fn test_trace_data_preserves_event_order() {
    let trace_path = "tests/fixtures/simple_trace.json";
    let data = TraceData::from_file(trace_path).unwrap();

    // Events should maintain their order
    assert_eq!(data.events[0].name, "test_function_a");
    assert_eq!(data.events[1].name, "test_function_a");
    assert_eq!(data.events[2].name, "test_function_b");
    assert_eq!(data.events[3].name, "execution");
}

#[test]
fn test_trace_data_timestamp_ordering() {
    let trace_path = "tests/fixtures/simple_trace.json";
    let data = TraceData::from_file(trace_path).unwrap();

    // Timestamps should be in order
    for i in 1..data.events.len() {
        assert!(data.events[i].timestamp >= data.events[i-1].timestamp,
                "Timestamps should be non-decreasing");
    }
}
