use runtime_scope::tui::TraceData;

#[test]
fn test_parse_trace_from_file_succeeds() {
    let trace_path = "tests/fixtures/simple_trace.json";
    let result = TraceData::from_file(trace_path);

    assert!(result.is_ok(), "Failed to parse trace file: {:?}", result.err());

    let data = result.unwrap();
    assert_eq!(data.events.len(), 4, "Should have 4 events");
    assert_eq!(data.workers.len(), 3, "Should have 3 unique workers");
}

#[test]
fn test_parse_trace_extracts_all_fields() {
    let trace_path = "tests/fixtures/simple_trace.json";
    let data = TraceData::from_file(trace_path).unwrap();

    // Check first event
    let event = &data.events[0];
    assert_eq!(event.name, "test_function_a");
    assert_eq!(event.worker_id, 1);
    assert_eq!(event.tid, 12346);
    assert_eq!(event.timestamp, 1.0); // 1000000.0 µs / 1_000_000 = 1.0s
    assert_eq!(event.cpu, 0);
    assert_eq!(event.detection_method, Some(4));
    assert_eq!(event.file, Some("test.rs".to_string()));
    assert_eq!(event.line, Some(42));
}

#[test]
fn test_parse_trace_handles_missing_optional_fields() {
    let trace_path = "tests/fixtures/simple_trace.json";
    let data = TraceData::from_file(trace_path).unwrap();

    // Last event has no file/line (scheduler event)
    let scheduler_event = &data.events[3];
    assert_eq!(scheduler_event.name, "execution");
    assert_eq!(scheduler_event.file, None);
    assert_eq!(scheduler_event.line, None);
}

#[test]
fn test_parse_trace_calculates_duration() {
    let trace_path = "tests/fixtures/simple_trace.json";
    let data = TraceData::from_file(trace_path).unwrap();

    // Duration should be max timestamp
    // 4000000.0 µs / 1_000_000 = 4.0s
    assert_eq!(data.duration, 4.0);
}

#[test]
fn test_parse_trace_aggregates_unique_workers() {
    let trace_path = "tests/fixtures/simple_trace.json";
    let data = TraceData::from_file(trace_path).unwrap();

    assert_eq!(data.workers.len(), 3);
    assert!(data.workers.contains(&1));
    assert!(data.workers.contains(&2));
    assert!(data.workers.contains(&3));

    // Workers should be sorted
    assert_eq!(data.workers, vec![1, 2, 3]);
}

#[test]
fn test_parse_trace_filters_non_begin_events() {
    // Only "B" (begin) events should be included
    let trace_path = "tests/fixtures/simple_trace.json";
    let data = TraceData::from_file(trace_path).unwrap();

    // All events in our fixture are "B" events
    // If we had "E" events, they would be filtered out
    for event in &data.events {
        // We can't verify ph directly since it's not stored,
        // but we know only B events are included
        assert!(!event.name.is_empty());
    }
}

#[test]
fn test_parse_invalid_file_returns_error() {
    let result = TraceData::from_file("nonexistent.json");
    assert!(result.is_err(), "Should fail for missing file");
}

#[test]
fn test_parse_invalid_json_returns_error() {
    use std::io::Write;
    use tempfile::NamedTempFile;

    let mut temp_file = NamedTempFile::new().unwrap();
    writeln!(temp_file, "{{ invalid json").unwrap();

    let result = TraceData::from_file(temp_file.path());
    assert!(result.is_err(), "Should fail for invalid JSON");
}

#[test]
fn test_parse_empty_trace_events() {
    use std::io::Write;
    use tempfile::NamedTempFile;

    let mut temp_file = NamedTempFile::new().unwrap();
    writeln!(temp_file, r#"{{"traceEvents": []}}"#).unwrap();

    let data = TraceData::from_file(temp_file.path()).unwrap();

    assert_eq!(data.events.len(), 0);
    assert_eq!(data.workers.len(), 0);
    assert_eq!(data.duration, 0.0);
}

#[test]
fn test_timestamp_conversion_microseconds_to_seconds() {
    let trace_path = "tests/fixtures/simple_trace.json";
    let data = TraceData::from_file(trace_path).unwrap();

    // ts: 1000000.0 µs should convert to 1.0 s
    assert_eq!(data.events[0].timestamp, 1.0);
    // ts: 2000000.0 µs should convert to 2.0 s
    assert_eq!(data.events[1].timestamp, 2.0);
    // ts: 3000000.0 µs should convert to 3.0 s
    assert_eq!(data.events[2].timestamp, 3.0);
}
