// String formatting intentionally uses format! for clarity
#![allow(clippy::format_push_string)]

use addr2line::Context;
use anyhow::{Context as _, Result};
use gimli::{EndianRcSlice, RunTimeEndian};
use object::{Object, ObjectSection};
use rustc_demangle::demangle;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::rc::Rc;

/// Symbolizer for resolving instruction pointers to source locations
///
/// Includes a cache to avoid re-resolving the same addresses repeatedly,
/// which significantly improves performance when symbolizing stack traces.
pub struct Symbolizer {
    ctx: Context<EndianRcSlice<RunTimeEndian>>,
    /// Cache of resolved frames by address
    cache: RefCell<HashMap<u64, ResolvedFrame>>,
}

impl Symbolizer {
    /// Create a new symbolizer for the given binary
    ///
    /// # Errors
    /// Returns an error if the binary file cannot be read or parsed, or if DWARF debug info is missing
    pub fn new<P: AsRef<Path>>(binary_path: P) -> Result<Self> {
        let binary_data = fs::read(binary_path.as_ref()).context("Failed to read binary file")?;

        let obj_file = object::File::parse(&*binary_data).context("Failed to parse object file")?;

        // Load DWARF debug info
        let endian =
            if obj_file.is_little_endian() { RunTimeEndian::Little } else { RunTimeEndian::Big };

        let load_section =
            |id: gimli::SectionId| -> Result<EndianRcSlice<RunTimeEndian>, gimli::Error> {
                let data = obj_file
                    .section_by_name(id.name())
                    .and_then(|section| section.uncompressed_data().ok())
                    .unwrap_or(std::borrow::Cow::Borrowed(&[][..]));
                Ok(EndianRcSlice::new(Rc::from(&*data), endian))
            };

        let dwarf = gimli::Dwarf::load(&load_section)?;
        let ctx = Context::from_dwarf(dwarf).context("Failed to load DWARF debug information")?;

        Ok(Self { ctx, cache: RefCell::new(HashMap::new()) })
    }

    /// Resolve an instruction pointer to source location information
    ///
    /// Uses a cache to avoid re-resolving the same address multiple times.
    pub fn resolve(&self, addr: u64) -> ResolvedFrame {
        // Check cache first
        if let Some(cached) = self.cache.borrow().get(&addr) {
            return cached.clone();
        }

        // Cache miss - perform actual resolution
        let mut result = Vec::new();

        if let Ok(mut frame_iter) = self.ctx.find_frames(addr).skip_all_loads() {
            while let Ok(Some(frame)) = frame_iter.next() {
                let function = frame
                    .function
                    .and_then(|f| f.demangle().ok().map(|s| s.to_string()))
                    .unwrap_or_else(|| "<unknown>".to_string());

                let location = frame.location.map(|loc| SourceLocation {
                    file: loc.file.map(std::string::ToString::to_string),
                    line: loc.line,
                    column: loc.column,
                });

                result.push(InlinedFrame { function, location });
            }
        }

        let resolved = ResolvedFrame {
            addr,
            frames: if result.is_empty() {
                vec![InlinedFrame { function: "<unknown>".to_string(), location: None }]
            } else {
                result
            },
        };

        // Store in cache
        self.cache.borrow_mut().insert(addr, resolved.clone());

        resolved
    }

    /// Demangle a Rust symbol name
    #[must_use]
    pub fn demangle_symbol(symbol: &str) -> String {
        format!("{:#}", demangle(symbol))
    }
}

/// A resolved stack frame (may contain multiple inlined frames)
#[derive(Debug, Clone)]
pub struct ResolvedFrame {
    pub addr: u64,
    pub frames: Vec<InlinedFrame>,
}

/// An inlined frame within a resolved frame
#[derive(Debug, Clone)]
pub struct InlinedFrame {
    pub function: String,
    pub location: Option<SourceLocation>,
}

/// Source code location
#[derive(Debug, Clone)]
pub struct SourceLocation {
    pub file: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

impl ResolvedFrame {
    /// Format the frame for display
    #[must_use]
    pub fn format(&self, frame_num: usize) -> String {
        let mut output = String::new();

        for (idx, inlined) in self.frames.iter().enumerate() {
            let prefix = if idx == 0 { format!("#{frame_num:<2}") } else { "    ".to_string() };

            output.push_str(&format!("{} 0x{:016x} {}", prefix, self.addr, inlined.function));

            if let Some(ref loc) = inlined.location {
                if let Some(ref file) = loc.file {
                    output.push_str(&format!("\n                      at {file}"));
                    if let Some(line) = loc.line {
                        output.push_str(&format!(":{line}"));
                        if let Some(col) = loc.column {
                            output.push_str(&format!(":{col}"));
                        }
                    }
                }
            }

            if idx < self.frames.len() - 1 {
                output.push('\n');
            }
        }

        output
    }
}
