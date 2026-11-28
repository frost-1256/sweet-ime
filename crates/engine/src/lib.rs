//! sweet-ime conversion engine.

#[cfg(windows)]
pub mod alpha_feedback;
pub mod app_modes;
pub mod converter;
pub mod dict;
pub mod english;
pub mod keymap;
pub mod learning;
pub mod live;
pub mod number;
pub mod paths;
pub mod reports;
pub mod romaji;
mod word_keys;
pub mod session;
pub mod simeji;
pub mod special;
pub mod text_file;
pub mod theme;
pub mod typo;
pub mod user_dict;
pub mod width;

pub use converter::{Conversion, Converter, Reranker, Segment, Word};
pub use dict::Dictionary;
pub use keymap::Keymap;
pub use learning::UserHistory;
pub use user_dict::UserDictionary;
pub use theme::{Theme, UserTheme};
pub use session::{CandidateList, Key, ListKind, Options, Preedit, Session, SpanAttr};
