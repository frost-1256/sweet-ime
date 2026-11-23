//! JNI bindings over the Sweet IME engine, used by the Android input method
//! (`android/`, class `io.github.sweetime.Native`).
//!
//! One `Context` per keyboard. Every call that changes the state returns a
//! snapshot string (see [`Context::snapshot`]); the Java side parses it and
//! updates the composing text and the candidate bar.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};

use sweet_engine::{Converter, Dictionary, Key, ListKind, Session, SpanAttr, paths};

mod ffi;
pub use ffi::*;

/// Key codes the Java side sends for keys that are not characters.
pub const KEY_BACKSPACE: i32 = -1;
pub const KEY_ENTER: i32 = -2;
pub const KEY_SPACE: i32 = -3;
pub const KEY_LEFT: i32 = -4;
pub const KEY_RIGHT: i32 = -5;
pub const KEY_ESCAPE: i32 = -6;
pub const KEY_EXPAND: i32 = -7;
/// Added to a letter typed with Shift.
pub const SHIFTED: i32 = 0x4000_0000;

/// Field and item separators in a snapshot.
const FIELD: char = '\u{1f}';
const ITEM: char = '\u{1e}';

static DICT_PATH: OnceLock<PathBuf> = OnceLock::new();

/// The converter shared by every keyboard in the process, rebuilt when the
/// zenz model is installed or removed.
static CONV: RwLock<Option<Shared>> = RwLock::new(None);

struct Shared {
    conv: Arc<Converter>,
    /// The model file the reranker was loaded from, if any.
    zenz: Option<PathBuf>,
}

/// The zenz model to use: installed and turned on in the settings.
fn wanted_zenz(neural: bool) -> Option<PathBuf> {
    paths::zenz_file().filter(|p| neural && p.is_file())
}

fn build(dict: &Path, zenz: Option<&Path>) -> Option<Converter> {
    let mut conv = paths::with_user_data(Converter::new(Dictionary::open(dict).ok()?));
    if let Some(z) = zenz.and_then(|p| sweet_zenz::Zenz::load(p).ok()) {
        conv = conv.with_reranker(Box::new(sweet_zenz::ZenzReranker::new(z)));
    }
    let o = paths::load_options();
    conv.apply_options(&o);
    Some(conv)
}

/// The shared converter; `None` before `init` names the dictionary.
pub fn converter() -> Option<Arc<Converter>> {
    if let Some(s) = CONV.read().unwrap_or_else(|e| e.into_inner()).as_ref() {
        return Some(s.conv.clone());
    }
    refresh_converter();
    CONV.read()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .map(|s| s.conv.clone())
}

/// Loads the converter, or loads it again when the model to use changed
/// (downloaded, removed, or AI conversion turned on or off), and applies
/// the settings.
pub fn refresh_converter() {
    let Some(dict) = DICT_PATH.get() else { return };
    let o = paths::load_options();
    let zenz = wanted_zenz(o.neural);
    let mut slot = CONV.write().unwrap_or_else(|e| e.into_inner());
    if let Some(s) = slot.as_ref() {
        if s.zenz == zenz {
            s.conv.apply_options(&o);
            return;
        }
    }
    // A model that fails to load leaves the dictionary-only converter.
    let conv = build(dict, zenz.as_deref());
    let loaded = conv.as_ref().is_some_and(|c| c.reranker_loaded());
    if let Some(conv) = conv {
        *slot = Some(Shared {
            conv: Arc::new(conv),
            zenz: zenz.filter(|_| loaded),
        });
    }
}

/// Converts `hf_dir` (config.json, model.safetensors, vocab.json,
/// merges.txt from Hugging Face) into the model file the IME loads, then
/// checks that it loads. Returns an error message on failure.
pub fn pack_zenz(hf_dir: &Path, out: &Path) -> Result<(), String> {
    let tmp = out.with_extension("swz.new");
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    sweet_zenz::Zenz::pack(hf_dir, &tmp).map_err(|e| e.to_string())?;
    let z = sweet_zenz::Zenz::load(&tmp).map_err(|e| e.to_string())?;
    if z.generate(&sweet_zenz::Zenz::prompt("きょう", ""), 4)
        .is_empty()
    {
        let _ = std::fs::remove_file(&tmp);
        return Err("the model produced no text".into());
    }
    drop(z);
    std::fs::rename(&tmp, out).map_err(|e| e.to_string())
}

/// The best conversion of `reading` (with the model when loaded): runs on
/// a background thread for live conversion.
pub fn refine(reading: &str, left_context: &str) -> Option<String> {
    let conv = converter()?;
    conv.refine_live(reading, left_context)
}

/// Points the per-user files at `files_dir` (the app's private storage)
/// and loads the dictionary at `dict`. Returns whether it loaded.
pub fn init(dict: PathBuf, files_dir: &str) -> bool {
    if DICT_PATH.get().is_none() {
        // SAFETY: called once, before the engine reads these variables.
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", files_dir);
            std::env::set_var("XDG_DATA_HOME", files_dir);
        }
        let _ = DICT_PATH.set(dict);
    }
    converter().is_some()
}

pub struct Context {
    session: Session,
    /// Candidates shown before the suggestions while typing: the text as
    /// shown and its reading, each committed as is when tapped.
    typed: Vec<String>,
    /// For each candidate in the last snapshot after `typed`, its index in
    /// the session's list (duplicates of `typed` are left out).
    listed: Vec<usize>,
    /// The field is a password or asks not to be learned from (a private
    /// browser tab); the session learns nothing there if the option is on.
    private_field: bool,
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}

impl Context {
    pub fn new() -> Self {
        let mut session = Session::new();
        session.options = paths::load_options();
        Self {
            session,
            typed: Vec::new(),
            listed: Vec::new(),
            private_field: false,
        }
    }

    pub fn set_private(&mut self, on: bool) {
        self.private_field = on;
        self.session.private = on && self.session.options.private_fields;
    }

    pub fn key_of(code: i32) -> Option<Key> {
        Some(match code {
            KEY_BACKSPACE => Key::Backspace,
            KEY_ENTER => Key::Enter,
            KEY_SPACE => Key::Space,
            KEY_LEFT => Key::Left,
            KEY_RIGHT => Key::Right,
            KEY_ESCAPE => Key::Escape,
            c if c >= 0 && c & SHIFTED != 0 => {
                Key::ShiftedLetter(char::from_u32((c & !SHIFTED) as u32)?)
            }
            c if c >= 0 => Key::Char(char::from_u32(c as u32)?),
            _ => return None,
        })
    }

    /// Handles a key; `None` when the app should get it.
    pub fn key(&mut self, code: i32) -> Option<String> {
        let conv = converter()?;
        let conv = &*conv;
        if code == KEY_EXPAND {
            return self.session.expand_predictions(conv).then(|| self.snapshot());
        }
        let key = Self::key_of(code)?;
        let typing = matches!(key, Key::Char(_) | Key::ShiftedLetter(_));
        if !self.session.is_active() && !typing {
            return None;
        }
        // With the list open, digits pick candidates on the desktop; on the
        // number row they type digits.
        let listed = self
            .session
            .candidates()
            .is_some_and(|l| l.kind == ListKind::Conversion);
        if listed && matches!(key, Key::Char(c) if c.is_ascii_digit()) {
            // The commit text stays queued for the snapshot.
            self.session.commit_all();
            self.session.handle(conv, key);
            return Some(self.snapshot());
        }
        if !self.session.handle(conv, key) {
            return None;
        }
        // A touch keyboard shows the candidates as soon as it converts.
        self.session.open_candidates(conv);
        Some(self.snapshot())
    }

    pub fn symbol(&mut self, ch: char) -> Option<String> {
        let conv = converter()?;
        self.session
            .handle_symbol(&conv, ch)
            .then(|| self.snapshot())
    }

    pub fn select(&mut self, index: usize) -> String {
        if let Some(text) = self.typed.get(index).cloned() {
            self.session.commit_as(&text);
            return self.snapshot();
        }
        let Some(&index) = self.listed.get(index - self.typed.len()) else {
            return self.snapshot();
        };
        if let Some(conv) = converter() {
            self.session.select_candidate(&conv, index);
            self.session.open_candidates(&conv);
        }
        self.snapshot()
    }

    /// What `typed` holds for the current state: nothing while converting
    /// or idle, else the preedit and (if it differs) the reading.
    fn typed_items(&self) -> Vec<String> {
        if self.session.is_converting() || !self.session.is_active() {
            return Vec::new();
        }
        let shown = self.session.preedit().text;
        if shown.is_empty() {
            return Vec::new();
        }
        let mut items = vec![shown];
        if let Some((reading, _)) = self.session.live_request() {
            if !items.contains(&reading) && !reading.is_empty() {
                items.push(reading);
            }
        }
        items
    }

    pub fn commit_all(&mut self) -> String {
        self.session.commit_all();
        self.snapshot()
    }

    /// Restores an idle composition after the editor verified the commit.
    pub fn undo_commit(&mut self, text: &str) -> Option<String> {
        if self.private_field || self.session.undoable_commit() != Some(text) {
            return None;
        }
        let conv = converter()?;
        if !self.session.handle(&conv, Key::UndoCommit) {
            return None;
        }
        self.session.take_undo();
        Some(self.snapshot())
    }

    pub fn reset(&mut self) {
        self.session.reset();
        self.session.take_commit();
        self.typed.clear();
        self.listed.clear();
    }

    /// Reads the settings file again (the settings screen wrote it).
    pub fn reload_options(&mut self) {
        refresh_converter();
        self.session.options = paths::load_options();
        self.set_private(self.private_field);
    }

    /// The live conversion to refine in the background, when the model is
    /// loaded: (reading, left context).
    pub fn live_request(&self) -> Option<(String, String)> {
        converter().filter(|c| c.has_reranker())?;
        self.session.live_request()
    }

    pub fn live_commit_request(&self) -> Option<(String, String)> {
        converter().filter(|c| c.has_reranker())?;
        self.session.live_commit_request()
    }

    /// Commits the result only for the same reading and editor context.
    pub fn commit_refined_live(
        &mut self,
        reading: &str,
        left: &str,
        surface: &str,
    ) -> Option<String> {
        let (current, context) = self.session.live_commit_request()?;
        if current != reading || context != left || surface.is_empty() {
            return None;
        }
        self.session.commit_as(surface);
        Some(self.snapshot())
    }

    /// Shows the refined live conversion if the reading is still the one
    /// typed; `None` when nothing changed.
    pub fn apply_live(&mut self, reading: &str, surface: &str) -> Option<String> {
        self.session
            .apply_live(reading, surface)
            .then(|| self.snapshot())
    }

    /// Whether a conversion is shown (Space picks the next candidate).
    pub fn converting(&self) -> bool {
        self.session
            .candidates()
            .is_some_and(|l| l.kind == ListKind::Conversion)
    }

    pub fn set_left_context(&mut self, text: &str) {
        self.session.set_left_context(text);
    }

    /// The state for the Java side, fields joined by U+001F: commit text,
    /// preedit, caret, spans, candidate kind, selected candidate, then the
    /// candidates joined by U+001E and a flag for an undoable commit.
    /// While typing, the candidates start with
    /// the text as shown and its reading. Offsets are UTF-16 units; spans are
    /// `start,end,attr` joined by `;` (attr 0 input, 1 converted, 2
    /// focused); kind is 0 for conversion and 1 for predictions; the
    /// selected candidate is -1 for predictions.
    pub fn snapshot(&mut self) -> String {
        let commit = self.session.take_commit();
        let undoable =
            !self.private_field && self.session.undoable_commit() == Some(commit.as_str());
        self.typed = self.typed_items();
        let p = self.session.preedit();
        let u16_at = |byte: usize| p.text[..byte.min(p.text.len())].encode_utf16().count();
        let spans: Vec<String> = p
            .spans
            .iter()
            .map(|&(a, b, attr)| {
                let attr = match attr {
                    SpanAttr::Input => 0,
                    SpanAttr::Converted => 1,
                    SpanAttr::Focused => 2,
                };
                format!("{},{},{attr}", u16_at(a), u16_at(b))
            })
            .collect();
        let (kind, selected, session_items) = match self.session.candidates() {
            Some(list) => match list.kind {
                ListKind::Conversion => (0, list.selected as i64, list.items),
                ListKind::Suggestion => (1, -1, list.items),
            },
            None => (1, -1, Vec::new()),
        };
        self.listed = (0..session_items.len())
            .filter(|&i| !self.typed.contains(&session_items[i]))
            .collect();
        let items: Vec<&String> = self
            .typed
            .iter()
            .chain(self.listed.iter().map(|&i| &session_items[i]))
            .collect();
        let clean = |s: &str| s.replace([FIELD, ITEM], "");
        let items: Vec<String> = items.iter().map(|s| clean(s)).collect();
        [
            clean(&commit),
            clean(&p.text),
            u16_at(p.caret).to_string(),
            spans.join(";"),
            kind.to_string(),
            selected.to_string(),
            items.join(&ITEM.to_string()),
            u8::from(undoable).to_string(),
        ]
        .join(&FIELD.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_codes_map_to_keys() {
        assert_eq!(Context::key_of('a' as i32), Some(Key::Char('a')));
        assert_eq!(
            Context::key_of('G' as i32 | SHIFTED),
            Some(Key::ShiftedLetter('G'))
        );
        assert_eq!(Context::key_of(KEY_SPACE), Some(Key::Space));
        assert_eq!(Context::key_of(-100), None);
    }

    #[test]
    fn types_and_commits_with_the_dictionary() {
        let dict = PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/system.dict"
        ));
        if !dict.exists() {
            return;
        }
        let dir = std::env::temp_dir().join("sweet-android-test");
        assert!(init(dict, dir.to_str().unwrap()));
        let mut c = Context::new();
        c.session.options.live_conversion = false;
        c.session.options.suggestions = false;
        c.session.options.undo_commit = true;
        let mut last = String::new();
        for ch in "kana".chars() {
            last = c.key(ch as i32).unwrap();
        }
        let fields: Vec<&str> = last.split(FIELD).collect();
        assert_eq!(fields[..3], ["", "かな", "2"]);
        let done = c.key(KEY_ENTER).unwrap();
        assert!(done.starts_with("かな\u{1f}\u{1f}0"));
        assert_eq!(done.split(FIELD).nth(7), Some("1"));
        assert!(c.undo_commit("別の文字").is_none());
        let restored = c.undo_commit("かな").unwrap();
        assert_eq!(
            restored.split(FIELD).take(3).collect::<Vec<_>>(),
            ["", "かな", "2"]
        );
        assert!(c.undo_commit("かな").is_none());
        c.reset();
        // Space converts and shows the candidates at once; a digit then
        // commits and types the digit.
        for ch in "kisya".chars() {
            c.key(ch as i32);
        }
        let shown = c.key(KEY_SPACE).unwrap();
        let fields: Vec<&str> = shown.split(FIELD).collect();
        assert_eq!(fields[4], "0");
        assert!(fields[6].split(ITEM).count() > 1, "{shown}");
        let typed = c.key('1' as i32).unwrap();
        let fields: Vec<&str> = typed.split(FIELD).collect();
        assert!(!fields[0].is_empty() && fields[1] == "1", "{typed}");
        c.reset();
        // Nothing to compose: the app gets Enter.
        assert_eq!(c.key(KEY_ENTER), None);
        // While typing, the text as shown and its reading come first and
        // commit as they are.
        c.session.options.live_conversion = true;
        for ch in "kawaii".chars() {
            c.key(ch as i32);
        }
        let typing = c.snapshot();
        let fields: Vec<&str> = typing.split(FIELD).collect();
        let items: Vec<&str> = fields[6].split(ITEM).collect();
        assert_eq!(items[0], fields[1], "{typing}");
        assert_eq!(items[1], "かわいい", "{typing}");
        let done = c.select(1);
        assert!(done.starts_with("かわいい\u{1f}\u{1f}0"), "{done}");
        assert_eq!(done.split(FIELD).nth(7), Some("1"));
        assert!(c.undo_commit("かわいい").is_some());
        c.reset();

        for ch in "kikai".chars() {
            c.key(ch as i32);
        }
        c.key(KEY_SPACE).unwrap();
        let done = c.select(0);
        let fields: Vec<_> = done.split(FIELD).collect();
        assert_eq!(fields[7], "1", "{done}");
        c.set_private(true);
        assert!(c.undo_commit(fields[0]).is_none());
        c.set_private(false);
        let restored = c.undo_commit(fields[0]).unwrap();
        assert_eq!(restored.split(FIELD).nth(1), Some("きかい"));
        c.reset();

        for ch in "kana".chars() {
            c.key(ch as i32);
        }
        c.key(KEY_ENTER).unwrap();
        c.reset();
        assert!(c.undo_commit("かな").is_none());
    }

    #[test]
    fn confirms_neural_result_for_the_current_reading() {
        let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let Ok(dict) = Dictionary::open(data.join("system.dict")) else {
            return;
        };
        let mut conv = Converter::new(dict);
        let model = sweet_zenz::Zenz::load(data.join("zenz-v3.1-xsmall.swz")).ok();
        let has_model = model.is_some();
        if let Some(model) = model {
            conv = conv.with_reranker(Box::new(sweet_zenz::ZenzReranker::new(model)));
        }
        let mut ctx = Context::new();
        ctx.session.options = sweet_engine::Options::default();
        ctx.session.set_left_context("資料を探しています。");
        for ch in "kinounokaiginoshiryou".chars() {
            ctx.session.handle(&conv, Key::Char(ch));
        }
        let (reading, left) = ctx.session.live_commit_request().unwrap();
        assert_eq!(reading, "きのうのかいぎのしりょう");
        let surface = if has_model {
            conv.refine_live(&reading, &left).unwrap()
        } else {
            "昨日の会議の資料".into()
        };
        assert_ne!(ctx.session.preedit().text, surface);
        assert!(
            ctx.commit_refined_live(&reading, "別の文脈", &surface)
                .is_none()
        );
        let snapshot = ctx.commit_refined_live(&reading, &left, &surface).unwrap();
        assert_eq!(snapshot.split(FIELD).next(), Some("昨日の会議の資料"));
        assert_eq!(snapshot.split(FIELD).nth(7), Some("1"));
        assert!(ctx.commit_refined_live(&reading, &left, &surface).is_none());

        for ch in "kouen".chars() {
            ctx.session.handle(&conv, Key::Char(ch));
        }
        assert_eq!(ctx.session.live_request().unwrap().0, "こうえ");
        assert_eq!(ctx.session.live_commit_request().unwrap().0, "こうえん");
        ctx.session.handle(&conv, Key::Space);
        assert!(ctx.session.live_commit_request().is_none());

        ctx.reset();
        for ch in "kouen".chars() {
            ctx.session.handle(&conv, Key::Char(ch));
        }
        let (preview, _) = ctx.session.live_request().unwrap();
        let (reading, left) = ctx.session.live_commit_request().unwrap();
        assert!(ctx.commit_refined_live(&preview, &left, "公演").is_none());
        let snapshot = ctx.commit_refined_live(&reading, &left, "公演").unwrap();
        assert_eq!(snapshot.split(FIELD).next(), Some("公演"));
    }
}
