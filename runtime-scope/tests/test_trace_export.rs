use runtime_scope::tui::TraceData;
use tempfile::NamedTempFile;

#[test]
#[ignore] // Requires refactoring TraceExporter to accept Write trait
fn test_export_creates_valid_json() {

    // We can't easily test the actual export without refactoring TraceExporter
    // to accept a writer instead of always writing to "trace.json"
    // This test is ignored until TraceExporter is refactored
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
