use hud::symbolization::Symbolizer;

#[test]
fn test_symbolizer_creation() {
    // Test that we can create a symbolizer for a binary
    let binary_path = env!("CARGO_BIN_EXE_hud");

    println!("Testing symbolizer creation on: {}", binary_path);

    let symbolizer = Symbolizer::new(binary_path);
    assert!(symbolizer.is_ok(), "Failed to create symbolizer: {:?}", symbolizer.err());

    println!("✅ Symbolizer created successfully");
}

#[test]
fn test_symbolizer_resolves_function_names() {
    // Test that the symbolizer can resolve addresses to function names
    let binary_path = env!("CARGO_BIN_EXE_hud");

    println!("Testing symbolization on: {}", binary_path);

    let symbolizer = Symbolizer::new(binary_path)
        .expect("Failed to create symbolizer");

    // Get function addresses from nm
    let nm_output = std::process::Command::new("nm")
        .args(&["-C", binary_path])
        .output()
        .expect("Failed to run nm");

    let symbols = String::from_utf8_lossy(&nm_output.stdout);

    // Try to find and resolve multiple addresses
    let mut found_valid_symbol = false;
    let mut attempts = 0;

    for line in symbols.lines() {
        if line.contains(" T ") && attempts < 5 {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if let Some(addr_str) = parts.first() {
                if let Ok(addr) = u64::from_str_radix(addr_str, 16) {
                    attempts += 1;
                    println!("\nTrying address: 0x{:x} ({})", addr, line);

                    let resolved = symbolizer.resolve(addr);
                    println!("  Resolved address: 0x{:x}", resolved.addr);

                    // Check if we got any frames
                    if !resolved.frames.is_empty() {
                        for (idx, frame) in resolved.frames.iter().enumerate() {
                            println!("  Frame {}: {}", idx, frame.function);

                            // As long as we got a function name (not "<unknown>"), that's good
                            if frame.function != "<unknown>" {
                                found_valid_symbol = true;

                                // If we also got source location, that's even better!
                                if let Some(ref loc) = frame.location {
                                    if let Some(ref file) = loc.file {
                                        if let Some(line) = loc.line {
                                            println!("  📍 {}:{}", file, line);
                                            println!("\n✅ SUCCESS: Full debug info available!");
                                            return;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // We should have found at least one valid symbol
    assert!(found_valid_symbol,
            "Symbolizer should resolve at least one address to a function name.\n\
             Tried {} addresses but all resolved to '<unknown>'.\n\
             This might indicate missing debug symbols.", attempts);

    println!("\n✅ SUCCESS: Symbolizer can resolve function names!");
}

#[test]
#[ignore] // Only run if you want to verify full debug info is available
fn test_dwarf_debug_info_available() {
    // This test verifies that DWARF debug info with file:line is available
    // It's ignored by default because it depends on build configuration

    let binary_path = env!("CARGO_BIN_EXE_hud");
    let symbolizer = Symbolizer::new(binary_path)
        .expect("Failed to create symbolizer");

    // Get function addresses
    let nm_output = std::process::Command::new("nm")
        .args(&["-C", binary_path])
        .output()
        .expect("Failed to run nm");

    let symbols = String::from_utf8_lossy(&nm_output.stdout);

    // Try multiple addresses
    for line in symbols.lines().take(10) {
        if line.contains(" T ") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if let Some(addr_str) = parts.first() {
                if let Ok(addr) = u64::from_str_radix(addr_str, 16) {
                    let resolved = symbolizer.resolve(addr);

                    for frame in &resolved.frames {
                        if let Some(ref loc) = frame.location {
                            if let (Some(ref file), Some(line)) = (&loc.file, loc.line) {
                                println!("✅ Found debug info: {} at {}:{}", frame.function, file, line);
                                return; // Success!
                            }
                        }
                    }
                }
            }
        }
    }

    panic!("No source location found - DWARF debug info not available in this build");
}
