use runtime_scope::symbolizer::Symbolizer;

#[test]
fn test_dwarf_file_line_extraction() {
    // Test that we can extract file:line from DWARF debug info

    // Use the runtime-scope binary itself as test subject
    let binary_path = env!("CARGO_BIN_EXE_runtime-scope");

    println!("Testing symbolization on: {}", binary_path);

    // Create symbolizer
    let symbolizer = Symbolizer::new(binary_path)
        .expect("Failed to create symbolizer");

    // Get a sample address from nm
    let nm_output = std::process::Command::new("nm")
        .args(&["-C", binary_path])
        .output()
        .expect("Failed to run nm");

    let symbols = String::from_utf8_lossy(&nm_output.stdout);

    // Find any function address (prefer one from our codebase, not std)
    let mut test_addr = None;
    for line in symbols.lines() {
        if line.contains(" T ") && (line.contains("runtime_scope") || line.contains("main")) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if let Some(addr_str) = parts.first() {
                if let Ok(addr) = u64::from_str_radix(addr_str, 16) {
                    test_addr = Some(addr);
                    println!("Testing with address: 0x{:x} ({})", addr, line);
                    break;
                }
            }
        }
    }

    let addr = test_addr.expect("Could not find test address");

    // Resolve the address
    let resolved = symbolizer.resolve(addr);

    println!("\n✅ Resolved frame:");
    println!("   Address: 0x{:x}", resolved.addr);

    for (idx, frame) in resolved.frames.iter().enumerate() {
        println!("\n   Frame {}: {}", idx, frame.function);

        if let Some(ref loc) = frame.location {
            if let Some(ref file) = loc.file {
                println!("   📍 File: {}", file);

                if let Some(line) = loc.line {
                    println!("   📍 Line: {}", line);

                    if let Some(col) = loc.column {
                        println!("   📍 Column: {}", col);
                    }

                    // Assert we got file:line info
                    assert!(!file.is_empty(), "File path should not be empty");
                    assert!(line > 0, "Line number should be > 0");

                    println!("\n✅ SUCCESS: Extracted file:line from DWARF!");
                    return;
                }
            }
        }
    }

    panic!("No source location found in resolved frames - DWARF info may be incomplete");
}
