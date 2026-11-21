//! sweet-ime conversion engine.

pub mod converter;
pub mod dict;
pub mod romaji;

pub use converter::{Conversion, Converter, Segment, Word};
pub use dict::Dictionary;
