//! Nano Cortex wire protocol: C304 builders + C305/protobuf parsers.
//!
//! Pure logic — no ESP-IDF. Firmware modules call [`builders`] / [`parsers`]
//! directly with slices and owned values.

pub mod builders;
pub mod editor;
pub mod parsers;
pub mod types;
pub mod varint;

pub use builders::*;
pub use parsers::*;
pub use types::*;
