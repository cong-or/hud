//! Frame origin classification for distinguishing user code from libraries.
//!
//! This module provides heuristics to classify stack frames as user code vs
//! library/runtime code. This is important because Rust statically links
//! dependencies (tokio, std, etc.) into the main executable, so memory
//! range checks alone can't distinguish user code from libraries.
//!
//! # Classification Strategy
//!
//! 1. **File path patterns** - Most reliable when DWARF info is available
//!    - `.cargo/registry/` → Third-party crate
//!    - `.rustup/toolchains/` → Rust toolchain (std, core, alloc)
//!    - `/rustc/` → Rust compiler runtime
//!
//! 2. **Function name prefixes** - Fallback when file paths unavailable
//!    - `std::`, `core::`, `alloc::` → Standard library
//!    - `tokio::`, `async_std::`, `futures::` → Async runtime
//!
//! 3. **Memory range** - Last resort for unresolved frames

/// Origin of a stack frame, used to distinguish user code from libraries.
///
/// The classification affects how frames are displayed in the TUI:
/// - User code is highlighted in green
/// - Library code is dimmed
/// - The topmost user frame gets special emphasis as the "entry point"
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FrameOrigin {
    /// User's application code (relative paths, no known library patterns)
    UserCode,
    /// Rust standard library (std, core, alloc)
    StdLib,
    /// Async runtime libraries (tokio, async-std, futures)
    RuntimeLib,
    /// Other third-party crates from cargo registry
    ThirdParty,
    /// Could not determine origin (no debug info, raw address)
    #[default]
    Unknown,
}

impl FrameOrigin {
    /// Returns true if this frame represents user application code.
    ///
    /// Used by the TUI to decide highlighting - user code gets green,
    /// everything else gets dimmed.
    #[must_use]
    pub fn is_user_code(&self) -> bool {
        matches!(self, FrameOrigin::UserCode)
    }
}

/// Classify a stack frame based on its function name and file path.
///
/// # Arguments
///
/// * `function` - Fully qualified function name (e.g., "tokio::runtime::spawn")
/// * `file` - Source file path from DWARF debug info, if available
/// * `in_executable` - Whether the address is within the main executable's memory range
///
/// # Classification Priority
///
/// 1. File path patterns (most reliable with debug info)
/// 2. Function name prefixes (works without debug info)
/// 3. Memory range fallback (least reliable for Rust)
///
/// # Examples
///
/// ```ignore
/// // User code - relative path, no library patterns
/// classify_frame("myapp::handler::process", Some("src/handler.rs"), true);
/// // → FrameOrigin::UserCode
///
/// // Tokio runtime - function prefix
/// classify_frame("tokio::runtime::scheduler::inject", None, true);
/// // → FrameOrigin::RuntimeLib
///
/// // Standard library - file path pattern
/// classify_frame("std::io::read", Some("/rustc/.../library/std/src/io/mod.rs"), true);
/// // → FrameOrigin::StdLib
/// ```
#[must_use]
pub fn classify_frame(function: &str, file: Option<&str>, in_executable: bool) -> FrameOrigin {
    // === UNKNOWN/UNRESOLVED CHECK ===
    // Frames we couldn't resolve should not be classified as user code
    if function == "<unknown>" || function.starts_with("0x") || function.starts_with("<library>") {
        return FrameOrigin::Unknown;
    }

    // === FILE PATH CLASSIFICATION ===
    // File paths are the most reliable signal when debug info is available
    if let Some(path) = file {
        // Cargo registry: third-party crates
        // e.g., /home/user/.cargo/registry/src/index.crates.io-xxx/tokio-1.0.0/src/runtime.rs
        if path.contains(".cargo/registry/") || path.contains(".cargo\\registry\\") {
            // Check if it's a known runtime crate
            if is_runtime_path(path) {
                return FrameOrigin::RuntimeLib;
            }
            return FrameOrigin::ThirdParty;
        }

        // Rustup toolchains: std, core, alloc
        // e.g., /home/user/.rustup/toolchains/stable-x86_64/lib/rustlib/src/rust/library/std/
        if path.contains(".rustup/toolchains/") || path.contains(".rustup\\toolchains\\") {
            return FrameOrigin::StdLib;
        }

        // Rust compiler sources: std implementation details
        // e.g., /rustc/abc123.../library/std/src/io/mod.rs
        if path.contains("/rustc/") || path.contains("\\rustc\\") {
            return FrameOrigin::StdLib;
        }

        // Absolute paths to system locations
        if path.starts_with("/usr/") || path.starts_with("/lib/") {
            return FrameOrigin::ThirdParty;
        }

        // Relative paths (src/, ., etc.) are likely user code
        if !path.starts_with('/') || path.starts_with("./") || path.starts_with("src/") {
            return FrameOrigin::UserCode;
        }

        // Absolute path that didn't match any library pattern - likely user code
        // e.g., /home/user/myproject/src/main.rs
        if path.starts_with('/') {
            return FrameOrigin::UserCode;
        }
    }

    // === FUNCTION NAME CLASSIFICATION ===
    // Fallback when file paths aren't available (stripped binaries, etc.)
    if let Some(origin) = classify_by_function_prefix(function) {
        return origin;
    }

    // === MEMORY RANGE FALLBACK ===
    // If we couldn't classify by name/path, use memory location
    if in_executable {
        // Inside main executable but couldn't identify - assume user code
        // This is the "optimistic" fallback for statically linked code
        return FrameOrigin::UserCode;
    }

    // Outside main executable (shared library)
    FrameOrigin::Unknown
}

// =============================================================================
// CLASSIFICATION TABLES
// =============================================================================

/// Standard library module prefixes
const STD_PREFIXES: &[&str] = &["std::", "core::", "alloc::"];

/// Async runtime crate prefixes (function names)
const RUNTIME_PREFIXES: &[&str] = &[
    "tokio::",
    "async_std::",
    "futures::",
    "futures_util::",
    "futures_core::",
    "mio::",
    "hyper::",
    "hyper_util::",
    "tower::",
    "tower_service::",
];

/// Common third-party crate prefixes (function names)
const THIRD_PARTY_PREFIXES: &[&str] = &[
    "serde::",
    "serde_json::",
    "tracing::",
    "log::",
    "regex::",
    "crossbeam::",
    "rayon::",
    "parking_lot::",
    "bcrypt::",
    "blowfish::",
    "flate2::",
    "axum::",
    "http::",
    "bytes::",
    "hashbrown::",
    "ahash::",
];

/// Runtime crate patterns in cargo registry paths
const RUNTIME_CRATE_PATTERNS: &[&str] = &[
    "/tokio-",
    "/async-std-",
    "/futures-",
    "/futures-util-",
    "/futures-core-",
    "/mio-",
    "/hyper-",
    "/hyper-util-",
    "/tower-",
    "/axum-",
    "/actix-",
    "/warp-",
];

// =============================================================================
// HELPER FUNCTIONS
// =============================================================================

/// Classify a function by its module prefix.
fn classify_by_function_prefix(function: &str) -> Option<FrameOrigin> {
    if STD_PREFIXES.iter().any(|p| function.starts_with(p)) {
        return Some(FrameOrigin::StdLib);
    }
    if RUNTIME_PREFIXES.iter().any(|p| function.starts_with(p)) {
        return Some(FrameOrigin::RuntimeLib);
    }
    if THIRD_PARTY_PREFIXES.iter().any(|p| function.starts_with(p)) {
        return Some(FrameOrigin::ThirdParty);
    }
    None
}

/// Check if a file path belongs to a known runtime crate.
fn is_runtime_path(path: &str) -> bool {
    RUNTIME_CRATE_PATTERNS.iter().any(|pattern| path.contains(pattern))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_code_relative_path() {
        let origin = classify_frame("myapp::main", Some("src/main.rs"), true);
        assert_eq!(origin, FrameOrigin::UserCode);
        assert!(origin.is_user_code());
    }

    #[test]
    fn test_tokio_by_function_name() {
        let origin = classify_frame("tokio::runtime::scheduler::inject::Inject::push", None, true);
        assert_eq!(origin, FrameOrigin::RuntimeLib);
        assert!(!origin.is_user_code());
    }

    #[test]
    fn test_std_by_rustc_path() {
        let origin = classify_frame(
            "std::io::Read::read",
            Some("/rustc/abc123def/library/std/src/io/mod.rs"),
            true,
        );
        assert_eq!(origin, FrameOrigin::StdLib);
    }

    #[test]
    fn test_cargo_registry_tokio() {
        let origin = classify_frame(
            "tokio::sync::mutex::Mutex::lock",
            Some(
                "/home/user/.cargo/registry/src/index.crates.io-xxx/tokio-1.35.0/src/sync/mutex.rs",
            ),
            true,
        );
        assert_eq!(origin, FrameOrigin::RuntimeLib);
    }

    #[test]
    fn test_cargo_registry_third_party() {
        let origin = classify_frame(
            "serde_json::de::from_str",
            Some("/home/user/.cargo/registry/src/index.crates.io-xxx/serde_json-1.0.0/src/de.rs"),
            true,
        );
        assert_eq!(origin, FrameOrigin::ThirdParty);
    }

    #[test]
    fn test_std_by_function_name() {
        let origin = classify_frame("std::thread::spawn", None, true);
        assert_eq!(origin, FrameOrigin::StdLib);
    }

    #[test]
    fn test_unknown_outside_executable() {
        let origin = classify_frame("0x7fff12345678", None, false);
        assert_eq!(origin, FrameOrigin::Unknown);
    }

    #[test]
    fn test_fallback_to_user_code() {
        // Unknown function but inside executable - assume user code
        let origin = classify_frame("my_custom_function", None, true);
        assert_eq!(origin, FrameOrigin::UserCode);
    }
}
