//! Domain types providing compile-time safety and self-documentation
//!
//! These newtype wrappers prevent common bugs like passing a TID where a
//! WorkerId is expected, and make function signatures more expressive.

use std::fmt;

/// Worker ID (0-indexed)
///
/// Represents a Tokio worker thread's logical ID (0, 1, 2, ...).
/// This is NOT the same as the thread ID (TID).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WorkerId(pub u32);

impl fmt::Display for WorkerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Worker#{}", self.0)
    }
}

/// Process ID
///
/// Represents a process ID in the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Pid(pub u32);

impl fmt::Display for Pid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PID:{}", self.0)
    }
}

impl From<i32> for Pid {
    fn from(pid: i32) -> Self {
        Pid(pid as u32)
    }
}

impl From<Pid> for i32 {
    fn from(pid: Pid) -> Self {
        pid.0 as i32
    }
}

/// Thread ID
///
/// Represents a thread ID in the system.
/// This is distinct from WorkerId - a thread has a TID assigned by the kernel,
/// while a worker has a logical ID assigned by Tokio.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Tid(pub u32);

impl fmt::Display for Tid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TID:{}", self.0)
    }
}

/// CPU ID
///
/// Represents a CPU core ID (0, 1, 2, ...).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CpuId(pub u32);

impl fmt::Display for CpuId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CPU:{}", self.0)
    }
}

/// Stack trace ID from eBPF
///
/// Represents a stack trace ID stored in the eBPF stack trace map.
/// Negative values indicate no stack trace was captured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StackId(pub i64);

impl StackId {
    /// Returns true if this stack ID is valid (non-negative)
    pub fn is_valid(self) -> bool {
        self.0 >= 0
    }

    /// Convert to u32 for eBPF map lookup (panics if invalid)
    pub fn as_map_key(self) -> u32 {
        assert!(self.is_valid(), "Cannot convert invalid StackId to map key");
        self.0 as u32
    }
}

/// Function name (validated, non-empty)
///
/// Represents a resolved function name from stack traces.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionName(String);

impl FunctionName {
    /// Create a new function name (panics if empty)
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        assert!(!name.is_empty(), "Function name cannot be empty");
        Self(name)
    }

    /// Get the function name as a string slice
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for FunctionName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for FunctionName {
    fn from(s: String) -> Self {
        FunctionName::new(s)
    }
}

impl From<&str> for FunctionName {
    fn from(s: &str) -> Self {
        FunctionName::new(s)
    }
}

/// Timestamp in nanoseconds
///
/// Represents an absolute point in time as nanoseconds since boot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Timestamp(pub u64);

impl Timestamp {
    /// Convert to seconds (f64)
    pub fn as_seconds(self) -> f64 {
        self.0 as f64 / 1_000_000_000.0
    }

    /// Convert to microseconds (u64)
    pub fn as_micros(self) -> u64 {
        self.0 / 1_000
    }

    /// Convert to milliseconds (f64)
    pub fn as_millis(self) -> f64 {
        self.0 as f64 / 1_000_000.0
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.3}s", self.as_seconds())
    }
}

/// Duration in nanoseconds
///
/// Represents a time duration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Duration(pub u64);

impl Duration {
    /// Convert to milliseconds (f64)
    pub fn as_millis(self) -> f64 {
        self.0 as f64 / 1_000_000.0
    }

    /// Convert to seconds (f64)
    pub fn as_seconds(self) -> f64 {
        self.0 as f64 / 1_000_000_000.0
    }

    /// Convert to microseconds (u64)
    pub fn as_micros(self) -> u64 {
        self.0 / 1_000
    }
}

impl fmt::Display for Duration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ms = self.as_millis();
        if ms >= 1000.0 {
            write!(f, "{:.2}s", self.as_seconds())
        } else {
            write!(f, "{:.2}ms", ms)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_worker_id_display() {
        let id = WorkerId(5);
        assert_eq!(id.to_string(), "Worker#5");
    }

    #[test]
    fn test_pid_conversion() {
        let pid = Pid::from(1234i32);
        assert_eq!(pid.0, 1234);
        let back: i32 = pid.into();
        assert_eq!(back, 1234);
    }

    #[test]
    fn test_stack_id_validity() {
        assert!(StackId(5).is_valid());
        assert!(!StackId(-1).is_valid());
        assert_eq!(StackId(42).as_map_key(), 42);
    }

    #[test]
    #[should_panic(expected = "Cannot convert invalid StackId")]
    fn test_invalid_stack_id_panics() {
        StackId(-1).as_map_key();
    }

    #[test]
    fn test_function_name() {
        let name = FunctionName::new("my_function");
        assert_eq!(name.as_str(), "my_function");
        assert_eq!(name.to_string(), "my_function");
    }

    #[test]
    #[should_panic(expected = "Function name cannot be empty")]
    fn test_empty_function_name_panics() {
        FunctionName::new("");
    }

    #[test]
    fn test_timestamp_conversions() {
        let ts = Timestamp(1_500_000_000); // 1.5 seconds
        assert_eq!(ts.as_seconds(), 1.5);
        assert_eq!(ts.as_millis(), 1500.0);
        assert_eq!(ts.as_micros(), 1_500_000);
    }

    #[test]
    fn test_duration_conversions() {
        let dur = Duration(5_000_000); // 5 milliseconds
        assert_eq!(dur.as_millis(), 5.0);
        assert_eq!(dur.as_seconds(), 0.005);
        assert_eq!(dur.as_micros(), 5_000);
    }

    #[test]
    fn test_duration_display() {
        assert_eq!(Duration(5_000_000).to_string(), "5.00ms");
        assert_eq!(Duration(1_500_000_000).to_string(), "1.50s");
    }
}
