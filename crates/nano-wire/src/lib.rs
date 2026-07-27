//! Nano Cortex wire protocol: C304 builders + C305/protobuf parsers.
//!
//! Pure logic — no ESP-IDF. Linked into the firmware via [`ffi`] C ABI so the
//! existing C call sites keep working while this crate owns the implementation.

pub mod builders;
pub mod ffi;
pub mod parsers;
pub mod types;
pub mod varint;

pub use builders::*;
pub use parsers::*;
pub use types::*;
