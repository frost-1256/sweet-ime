//! JNI bindings over the Sweet IME engine, used by the Android input method
//! (`android/`, class `io.github.sweetime.Native`).
//!
//! One `Context` per keyboard. Every call that changes the state returns a
//! snapshot string (see [`Context::snapshot`]); the Java side parses it and
//! updates the composing text and the candidate bar.

use std::path::PathBuf;
use std::sync::OnceLock;

use jni::JNIEnv;
use jni::objects::{JClass, JString};
use jni::sys::{jboolean, jint, jlong, jstring};
use sweet_engine::{Converter, Dictionary, Key, ListKind, Session, SpanAttr, paths};

/// Key codes the Java side sends for keys that are not characters.
pub const KEY_BACKSPACE: i32 = -1;
pub const KEY_ENTER: i32 = -2;
pub const KEY_SPACE: i32 = -3;
pub const KEY_LEFT: i32 = -4;
pub const KEY_RIGHT: i32 = -5;
pub const KEY_ESCAPE: i32 = -6;
/// Added to a letter typed with Shift.
pub const SHIFTED: i32 = 0x4000_0000;

/// Field and item separators in a snapshot.
const FIELD: char = '\u{1f}';
const ITEM: char = '\u{1e}';

static DICT_PATH: OnceLock<PathBuf> = OnceLock::new();

/// The converter shared by every keyboard in the process.
pub fn converter() -> Option<&'static Converter> {
    static CONV: OnceLock<Option<Converter>> = OnceLock::new();
    // Not loaded before `init` names the dictionary.
    let dict = DICT_PATH.get()?;
    CONV.get_or_init(|| {
        let mut conv = Dictionary::open(dict).ok().map(Converter::new)?;
        if let Some(p) = paths::history_file() {
            conv = conv.with_history(sweet_engine::UserHistory::with_file(p));
        }
        if let Some(p) = paths::user_dict_file() {
            conv = conv.with_user_dict(sweet_engine::UserDictionary::with_file(p));
        }
        let o = paths::load_options();
        conv.configure(false, o.learning);
        conv.set_english_words(o.english_words, o.english_katakana);
        Some(conv)
    })
    .as_ref()
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
        Self { session }
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

    pub fn select(&mut self, index: usize) -> String {
        if let Some(conv) = converter() {
            self.session.select_candidate(conv, index);
            self.session.open_candidates(conv);
        }
        self.snapshot()
    }

    pub fn commit_all(&mut self) -> String {
        self.session.commit_all();
        self.snapshot()
    }

    pub fn reset(&mut self) {
        self.session.reset();
        self.session.take_commit();
    }

    pub fn set_left_context(&mut self, text: &str) {
        self.session.set_left_context(text);
    }

    /// The state for the Java side, fields joined by U+001F: commit text,
    /// preedit, caret, spans, candidate kind, selected candidate, then the
    /// candidates joined by U+001E. Offsets are UTF-16 units; spans are
    /// `start,end,attr` joined by `;` (attr 0 input, 1 converted, 2
    /// focused); kind is 0 for conversion and 1 for predictions; the
    /// selected candidate is -1 for predictions.
    pub fn snapshot(&mut self) -> String {
        let commit = self.session.take_commit();
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
        let (kind, selected, items) = match self.session.candidates() {
            Some(list) => match list.kind {
                ListKind::Conversion => (0, list.selected as i64, list.items),
                ListKind::Suggestion => (1, -1, list.items),
            },
            None => (0, -1, Vec::new()),
        };
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
        ]
        .join(&FIELD.to_string())
    }
}

// ---- JNI -----------------------------------------------------------------

fn guard<T>(default: T, f: impl FnOnce() -> T) -> T {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or(default)
}

fn to_string(env: &mut JNIEnv, s: &JString) -> String {
    env.get_string(s).map(Into::into).unwrap_or_default()
}

fn to_java(env: &mut JNIEnv, s: Option<String>) -> jstring {
    match s.and_then(|s| env.new_string(s).ok()) {
        Some(s) => s.into_raw(),
        None => std::ptr::null_mut(),
    }
}

unsafe fn ctx<'a>(p: jlong) -> &'a mut Context {
    unsafe { &mut *(p as *mut Context) }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_sweetime_Native_init(
    mut env: JNIEnv,
    _: JClass,
    dict: JString,
    files_dir: JString,
) -> jboolean {
    let dict = to_string(&mut env, &dict);
    let files = to_string(&mut env, &files_dir);
    guard(false, || init(PathBuf::from(dict), &files)) as jboolean
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_sweetime_Native_create(_: JNIEnv, _: JClass) -> jlong {
    guard(0, || Box::into_raw(Box::new(Context::new())) as jlong)
}

/// # Safety
/// `p` comes from `create` and is not used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_sweetime_Native_free(_: JNIEnv, _: JClass, p: jlong) {
    if p != 0 {
        drop(unsafe { Box::from_raw(p as *mut Context) });
    }
}

/// # Safety
/// `p` is a live context.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_sweetime_Native_key(
    mut env: JNIEnv,
    _: JClass,
    p: jlong,
    code: jint,
) -> jstring {
    let c = unsafe { ctx(p) };
    let s = guard(None, || c.key(code));
    to_java(&mut env, s)
}

/// # Safety
/// `p` is a live context.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_sweetime_Native_select(
    mut env: JNIEnv,
    _: JClass,
    p: jlong,
    index: jint,
) -> jstring {
    let c = unsafe { ctx(p) };
    let s = guard(None, || Some(c.select(index.max(0) as usize)));
    to_java(&mut env, s)
}

/// # Safety
/// `p` is a live context.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_sweetime_Native_commitAll(
    mut env: JNIEnv,
    _: JClass,
    p: jlong,
) -> jstring {
    let c = unsafe { ctx(p) };
    let s = guard(None, || Some(c.commit_all()));
    to_java(&mut env, s)
}

/// # Safety
/// `p` is a live context.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_sweetime_Native_reset(_: JNIEnv, _: JClass, p: jlong) {
    let c = unsafe { ctx(p) };
    guard((), || c.reset());
}

/// # Safety
/// `p` is a live context.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_sweetime_Native_setLeftContext(
    mut env: JNIEnv,
    _: JClass,
    p: jlong,
    text: JString,
) {
    let c = unsafe { ctx(p) };
    let t = to_string(&mut env, &text);
    guard((), || c.set_left_context(&t));
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
        let mut last = String::new();
        for ch in "kana".chars() {
            last = c.key(ch as i32).unwrap();
        }
        let fields: Vec<&str> = last.split(FIELD).collect();
        assert_eq!(fields[..3], ["", "かな", "2"]);
        let done = c.key(KEY_ENTER).unwrap();
        assert!(done.starts_with("かな\u{1f}\u{1f}0"));
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
    }
}
