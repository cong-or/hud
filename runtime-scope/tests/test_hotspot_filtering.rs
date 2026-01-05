use runtime_scope::tui::{TraceData, hotspot::HotspotView};

#[test]
fn test_hotspot_aggregates_events_by_function_name() {
    let trace_path = "tests/fixtures/simple_trace.json";
    let data = TraceData::from_file(trace_path).unwrap();
    let view = HotspotView::new(&data);

    // Should aggregate 2x test_function_a, 1x test_function_b, 1x execution
    let hotspots = view.hotspots;
    assert_eq!(hotspots.len(), 3);
}

#[test]
fn test_hotspot_calculates_correct_counts() {
    let trace_path = "tests/fixtures/simple_trace.json";
    let data = TraceData::from_file(trace_path).unwrap();
    let view = HotspotView::new(&data);

    // Find test_function_a
    let func_a = view.hotspots.iter()
        .find(|h| h.name == "test_function_a")
        .expect("Should find test_function_a");

    assert_eq!(func_a.count, 2, "test_function_a should have 2 samples");
}

#[test]
fn test_hotspot_calculates_correct_percentages() {
    let trace_path = "tests/fixtures/simple_trace.json";
    let data = TraceData::from_file(trace_path).unwrap();
    let view = HotspotView::new(&data);

    // Total events: 4
    // test_function_a: 2/4 = 50%
    let func_a = view.hotspots.iter()
        .find(|h| h.name == "test_function_a")
        .expect("Should find test_function_a");

    assert!((func_a.percentage - 50.0).abs() < 0.1,
            "Expected 50%, got {}", func_a.percentage);
}

#[test]
fn test_hotspot_sorts_by_count_descending() {
    let trace_path = "tests/fixtures/simple_trace.json";
    let data = TraceData::from_file(trace_path).unwrap();
    let view = HotspotView::new(&data);

    // First hotspot should have highest count
    for i in 1..view.hotspots.len() {
        assert!(view.hotspots[i-1].count >= view.hotspots[i].count,
                "Hotspots should be sorted by count descending");
    }
}

#[test]
fn test_hotspot_filter_by_name_case_insensitive() {
    let trace_path = "tests/fixtures/simple_trace.json";
    let data = TraceData::from_file(trace_path).unwrap();
    let mut view = HotspotView::new(&data);

    // Filter for "FUNCTION" (uppercase) should match "test_function_a" and "test_function_b"
    view.apply_filter("FUNCTION");

    assert_eq!(view.hotspots.len(), 2, "Should match 2 functions");
    assert!(view.hotspots.iter().all(|h| h.name.to_lowercase().contains("function")));
    assert!(view.is_filtered());
}

#[test]
fn test_hotspot_filter_by_name_substring() {
    let trace_path = "tests/fixtures/simple_trace.json";
    let data = TraceData::from_file(trace_path).unwrap();
    let mut view = HotspotView::new(&data);

    // Filter for "_a" should match only "test_function_a"
    view.apply_filter("_a");

    assert_eq!(view.hotspots.len(), 1);
    assert_eq!(view.hotspots[0].name, "test_function_a");
}

#[test]
fn test_hotspot_filter_empty_query_shows_all() {
    let trace_path = "tests/fixtures/simple_trace.json";
    let data = TraceData::from_file(trace_path).unwrap();
    let mut view = HotspotView::new(&data);

    let original_count = view.hotspots.len();

    view.apply_filter("");

    assert_eq!(view.hotspots.len(), original_count);
    assert!(!view.is_filtered(), "Empty filter should not be active");
}

#[test]
fn test_hotspot_filter_no_matches_returns_empty() {
    let trace_path = "tests/fixtures/simple_trace.json";
    let data = TraceData::from_file(trace_path).unwrap();
    let mut view = HotspotView::new(&data);

    view.apply_filter("nonexistent_function_xyz");

    assert_eq!(view.hotspots.len(), 0);
    assert!(view.is_filtered());
}

#[test]
fn test_hotspot_clear_filter_restores_original() {
    let trace_path = "tests/fixtures/simple_trace.json";
    let data = TraceData::from_file(trace_path).unwrap();
    let mut view = HotspotView::new(&data);

    let original_count = view.hotspots.len();

    // Apply filter
    view.apply_filter("function_a");
    assert!(view.hotspots.len() < original_count);

    // Clear filter
    view.clear_filter();
    assert_eq!(view.hotspots.len(), original_count);
    assert!(!view.is_filtered());
}

#[test]
fn test_hotspot_filter_by_workers_single_worker() {
    let trace_path = "tests/fixtures/simple_trace.json";
    let data = TraceData::from_file(trace_path).unwrap();
    let mut view = HotspotView::new(&data);

    // Filter to only worker 1
    view.filter_by_workers(&[1], &data);

    // Should only have test_function_a (2 events from worker 1)
    assert!(view.is_filtered());
    assert_eq!(view.hotspots.len(), 1);
    assert_eq!(view.hotspots[0].name, "test_function_a");
    assert_eq!(view.hotspots[0].count, 2);
}

#[test]
fn test_hotspot_filter_by_workers_multiple_workers() {
    let trace_path = "tests/fixtures/simple_trace.json";
    let data = TraceData::from_file(trace_path).unwrap();
    let mut view = HotspotView::new(&data);

    // Filter to workers 1 and 2
    view.filter_by_workers(&[1, 2], &data);

    assert!(view.is_filtered());
    // Should have test_function_a and test_function_b
    assert_eq!(view.hotspots.len(), 2);
}

#[test]
fn test_hotspot_filter_by_workers_recalculates_percentages() {
    let trace_path = "tests/fixtures/simple_trace.json";
    let data = TraceData::from_file(trace_path).unwrap();
    let mut view = HotspotView::new(&data);

    // Filter to only worker 1 (2 events total)
    view.filter_by_workers(&[1], &data);

    // test_function_a should be 100% (2/2)
    let func_a = &view.hotspots[0];
    assert!((func_a.percentage - 100.0).abs() < 0.1,
            "Expected 100%, got {}", func_a.percentage);
}

#[test]
fn test_hotspot_filter_by_all_workers_clears_filter() {
    let trace_path = "tests/fixtures/simple_trace.json";
    let data = TraceData::from_file(trace_path).unwrap();
    let mut view = HotspotView::new(&data);

    let original_count = view.hotspots.len();

    // Filter by all workers should be same as no filter
    view.filter_by_workers(&data.workers, &data);

    assert_eq!(view.hotspots.len(), original_count);
    assert!(!view.is_filtered());
}

#[test]
fn test_hotspot_scroll_up_decrements_selection() {
    let trace_path = "tests/fixtures/simple_trace.json";
    let data = TraceData::from_file(trace_path).unwrap();
    let mut view = HotspotView::new(&data);

    view.scroll_down();
    view.scroll_down();
    assert_eq!(view.selected_index, 2);

    view.scroll_up();
    assert_eq!(view.selected_index, 1);
}

#[test]
fn test_hotspot_scroll_down_increments_selection() {
    let trace_path = "tests/fixtures/simple_trace.json";
    let data = TraceData::from_file(trace_path).unwrap();
    let mut view = HotspotView::new(&data);

    assert_eq!(view.selected_index, 0);

    view.scroll_down();
    assert_eq!(view.selected_index, 1);
}

#[test]
fn test_hotspot_scroll_up_stops_at_zero() {
    let trace_path = "tests/fixtures/simple_trace.json";
    let data = TraceData::from_file(trace_path).unwrap();
    let mut view = HotspotView::new(&data);

    assert_eq!(view.selected_index, 0);

    view.scroll_up();
    assert_eq!(view.selected_index, 0, "Should not go below 0");
}

#[test]
fn test_hotspot_scroll_down_stops_at_end() {
    let trace_path = "tests/fixtures/simple_trace.json";
    let data = TraceData::from_file(trace_path).unwrap();
    let mut view = HotspotView::new(&data);

    let max_index = view.hotspots.len() - 1;

    // Scroll past the end
    for _ in 0..100 {
        view.scroll_down();
    }

    assert_eq!(view.selected_index, max_index, "Should not go past last item");
}

#[test]
fn test_hotspot_get_selected_returns_current_item() {
    let trace_path = "tests/fixtures/simple_trace.json";
    let data = TraceData::from_file(trace_path).unwrap();
    let mut view = HotspotView::new(&data);

    let first = view.get_selected().unwrap();
    let first_name = first.name.clone();

    view.scroll_down();
    let second = view.get_selected().unwrap();

    assert_ne!(first_name, second.name, "Selected item should change after scroll");
}

#[test]
fn test_hotspot_preserves_file_line_info() {
    let trace_path = "tests/fixtures/simple_trace.json";
    let data = TraceData::from_file(trace_path).unwrap();
    let view = HotspotView::new(&data);

    let func_a = view.hotspots.iter()
        .find(|h| h.name == "test_function_a")
        .expect("Should find test_function_a");

    assert_eq!(func_a.file, Some("test.rs".to_string()));
    assert_eq!(func_a.line, Some(42));
}

#[test]
fn test_hotspot_tracks_worker_distribution() {
    let trace_path = "tests/fixtures/simple_trace.json";
    let data = TraceData::from_file(trace_path).unwrap();
    let view = HotspotView::new(&data);

    let func_a = view.hotspots.iter()
        .find(|h| h.name == "test_function_a")
        .expect("Should find test_function_a");

    // Both samples are from worker 1
    assert_eq!(func_a.workers.get(&1), Some(&2));
    assert_eq!(func_a.workers.len(), 1);
}
