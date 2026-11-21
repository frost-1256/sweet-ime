//! sweet-ime conversion engine.

pub mod converter;
pub mod dict;
pub mod romaji;
pub mod session;
pub mod width;

pub use converter::{Conversion, Converter, Segment, Word};
pub use dict::Dictionary;
pub use session::{CandidateList, Key, Preedit, Session, SpanAttr};
