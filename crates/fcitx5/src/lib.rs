//! C ABI over the Sweet IME engine, used by the fcitx5 addon (`cpp/`).
//!
//! One `SweetContext` per input context. The frontend feeds key events with
//! X keysyms, which are mapped to the Windows virtual-key codes the key map
//! uses, so `keymap.txt` is shared across platforms. After every call that
//! changes the state the frontend reads the snapshot (commit text, preedit,
//! candidate page) through the getters; returned strings stay valid until
//! the next call on the same context.
//!
//! Live conversion is refined by the neural reranker on a worker thread.
//! When a result is ready, the notify callback is called on the worker
//! thread; the frontend then calls `sweet_apply_live` on its main thread.

use std::ffi::{CStr, CString, c_char, c_void};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

use sweet_engine::keymap::{Action, Chord, Keymap};
use sweet_engine::{Converter, Dictionary, Key, ListKind, Session, SpanAttr, paths};

mod keysym;
mod live;

pub const MOD_SHIFT: u32 = 1;
pub const MOD_CTRL: u32 = 2;
pub const MOD_ALT: u32 = 4;

pub const RESULT_CONSUMED: u32 = 1;
/// The IME was switched on or off.
pub const RESULT_MODE_CHANGED: u32 = 2;

/// Where the system dictionary is looked up, first match wins.
fn dictionary_paths() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Ok(p) = std::env::var("SWEET_IME_DICT") {
        v.push(PathBuf::from(p));
    }
    if let Some(dir) = option_env!("SWEET_IME_DATADIR") {
        v.push(PathBuf::from(dir).join("system.dict"));
    }
    if let Some(d) = paths::data_dir() {
        v.push(d.join("system.dict"));
    }
    let dirs =
        std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    for d in dirs.split(':').filter(|d| !d.is_empty()) {
        v.push(PathBuf::from(d).join("sweet-ime").join("system.dict"));
    }
    v
}

/// The converter shared by every input context in the process.
pub fn converter() -> Option<&'static Converter> {
    static CONV: OnceLock<Option<Converter>> = OnceLock::new();
    CONV.get_or_init(|| {
        let mut conv = dictionary_paths()
            .into_iter()
            .find_map(|p| Dictionary::open(&p).ok())
            .map(Converter::new)?;
        if let Some(p) = paths::history_file() {
            conv = conv.with_history(sweet_engine::UserHistory::with_file(p));
        }
        if let Some(p) = sweet_engine::paths::user_dict_file() {
            conv = conv.with_user_dict(sweet_engine::UserDictionary::with_file(p));
        }
        Some(match load_zenz() {
            Some(r) => conv.with_reranker(Box::new(r)),
            None => conv,
        })
    })
    .as_ref()
}

/// Optional neural reranker (not bundled; see `get-zenz.sh`).
fn load_zenz() -> Option<sweet_zenz::ZenzReranker> {
    let mut v = Vec::new();
    if let Ok(p) = std::env::var("SWEET_IME_ZENZ") {
        v.push(PathBuf::from(p));
    }
    v.extend(paths::zenz_file());
    v.into_iter()
        .find_map(|p| sweet_zenz::Zenz::load(&p).ok())
        .map(sweet_zenz::ZenzReranker::new)
}

fn settings_stamp() -> [Option<SystemTime>; 2] {
    let mtime = |p: Option<PathBuf>| {
        p.and_then(|p| std::fs::metadata(p).ok())
            .and_then(|m| m.modified().ok())
    };
    [mtime(paths::config_file()), mtime(paths::keymap_file())]
}

type NotifyFn = extern "C" fn(*mut c_void);

struct Notify {
    f: NotifyFn,
    user: usize,
}

/// Shared with the live worker: the latest refined result and how to wake
/// the frontend.
#[derive(Default)]
struct LiveShared {
    result: Mutex<Option<(String, String)>>,
    notify: Mutex<Option<Notify>>,
}

#[derive(Default)]
struct Snapshot {
    commit: CString,
    preedit: CString,
    caret: u32,
    spans: Vec<(u32, u32, u32)>,
    candidates: Vec<CString>,
    cursor: i32,
    page_start: u32,
    total: u32,
    kind: u32,
}

pub struct SweetContext {
    session: Session,
    keymap: Keymap,
    open: bool,
    stamp: [Option<SystemTime>; 2],
    live: Arc<LiveShared>,
    snap: Snapshot,
}

/// The frontend's view of a key event.
pub enum Input {
    Toggle(Option<bool>),
    Key(Key),
}

impl SweetContext {
    fn new() -> Self {
        let mut c = Self::bare();
        c.reload_settings();
        c
    }

    /// Built-in settings, no files read.
    fn bare() -> Self {
        Self {
            session: Session::new(),
            keymap: Keymap::default(),
            open: true,
            stamp: [None, None],
            live: Arc::default(),
            snap: Snapshot::default(),
        }
    }

    fn reload_settings(&mut self) {
        self.stamp = settings_stamp();
        let o = paths::load_options();
        if let Some(c) = converter() {
            c.configure(o.neural, o.learning);
            c.set_english_words(o.english_words, o.english_katakana);
        }
        self.session.options = o;
        self.keymap = paths::load_keymap();
    }

    fn reload_settings_if_changed(&mut self) {
        if settings_stamp() != self.stamp {
            self.reload_settings();
        }
    }

    /// Maps a key event to an IME action; `None` lets the application have it.
    pub fn translate(&self, sym: u32, unicode: u32, mods: u32) -> Option<Input> {
        let (ctrl, shift, alt) = (
            mods & MOD_CTRL != 0,
            mods & MOD_SHIFT != 0,
            mods & MOD_ALT != 0,
        );
        if let Some(vk) = keysym::to_vk(sym) {
            let chord = Chord {
                vk,
                ctrl,
                shift,
                alt,
            };
            if let Some(action) = self.keymap.lookup(chord, self.session.is_active()) {
                return Some(match action {
                    Action::ImeToggle => Input::Toggle(None),
                    Action::ImeOn => Input::Toggle(Some(true)),
                    Action::ImeOff => Input::Toggle(Some(false)),
                    Action::Key(k) => Input::Key(k),
                });
            }
        }
        if ctrl || alt {
            return None;
        }
        let c = char::from_u32(unicode)?;
        if c.is_control() || !c.is_ascii() || c == ' ' {
            return None;
        }
        Some(Input::Key(if shift && c.is_ascii_alphabetic() {
            Key::ShiftedLetter(c)
        } else {
            Key::Char(c)
        }))
    }

    fn allows(&self, key: Key) -> bool {
        if converter().is_none() || !self.open {
            return false;
        }
        self.session.is_active() || matches!(key, Key::Char(_) | Key::ShiftedLetter(_) | Key::Space)
    }

    fn process_key(&mut self, sym: u32, unicode: u32, mods: u32) -> u32 {
        self.reload_settings_if_changed();
        let key = match self.translate(sym, unicode, mods) {
            None => return 0,
            Some(Input::Toggle(t)) => {
                let new = t.unwrap_or(!self.open);
                if !new && self.session.is_active() {
                    self.session.commit_all();
                }
                let changed = new != self.open;
                self.open = new;
                self.snapshot();
                return RESULT_CONSUMED | if changed { RESULT_MODE_CHANGED } else { 0 };
            }
            Some(Input::Key(k)) => k,
        };
        if !self.allows(key) {
            return 0;
        }
        let Some(conv) = converter() else { return 0 };
        if !self.session.handle(conv, key) {
            return 0;
        }
        self.snapshot();
        self.request_live_refinement(conv);
        RESULT_CONSUMED
    }

    fn request_live_refinement(&self, conv: &Converter) {
        if !conv.has_reranker() {
            return;
        }
        let Some((reading, left_context)) = self.session.live_request() else {
            return;
        };
        let shared = Arc::downgrade(&self.live);
        let r2 = reading.clone();
        live::submit(live::Job {
            reading,
            left_context,
            done: Box::new(move |surface| {
                let Some(shared) = shared.upgrade() else {
                    return;
                };
                *shared.result.lock().unwrap_or_else(|e| e.into_inner()) = Some((r2, surface));
                // Called under the lock so `sweet_context_free` cannot
                // complete while the frontend is being notified.
                let n = shared.notify.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(n) = n.as_ref() {
                    (n.f)(n.user as *mut c_void);
                }
            }),
        });
    }

    fn apply_live(&mut self) -> bool {
        let r = self
            .live
            .result
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        let Some((reading, surface)) = r else {
            return false;
        };
        if !self.session.apply_live(&reading, &surface) {
            return false;
        }
        self.snapshot();
        true
    }

    /// Captures the session state for the getters (takes the commit text).
    fn snapshot(&mut self) {
        let cs = |s: &str| CString::new(s.replace('\0', "")).unwrap_or_default();
        let s = &mut self.snap;
        let commit = self.session.take_commit();
        s.commit = cs(&commit);
        let p = self.session.preedit();
        s.preedit = cs(&p.text);
        s.caret = p.caret as u32;
        s.spans = p
            .spans
            .iter()
            .map(|&(a, b, attr)| {
                let attr = match attr {
                    SpanAttr::Input => 0,
                    SpanAttr::Converted => 1,
                    SpanAttr::Focused => 2,
                };
                (a as u32, b as u32, attr)
            })
            .collect();
        s.candidates.clear();
        s.cursor = -1;
        s.page_start = 0;
        s.total = 0;
        s.kind = 0;
        if let Some(list) = self.session.candidates() {
            let start = list.page_start();
            let end = (start + list.page_size).min(list.items.len());
            s.candidates = list.items[start..end].iter().map(|t| cs(t)).collect();
            s.page_start = start as u32;
            s.total = list.items.len() as u32;
            match list.kind {
                ListKind::Conversion => s.cursor = (list.selected - start) as i32,
                ListKind::Suggestion => s.kind = 1,
            }
        }
    }
}

// ---- C ABI ---------------------------------------------------------------

unsafe fn ctx<'a>(p: *mut SweetContext) -> &'a mut SweetContext {
    unsafe { &mut *p }
}

fn guard<T>(default: T, f: impl FnOnce() -> T) -> T {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or(default)
}

/// Loads the dictionary (and the model) now; returns whether it was found.
#[unsafe(no_mangle)]
pub extern "C" fn sweet_init() -> bool {
    guard(false, || converter().is_some())
}

#[unsafe(no_mangle)]
pub extern "C" fn sweet_context_new() -> *mut SweetContext {
    guard(std::ptr::null_mut(), || {
        Box::into_raw(Box::new(SweetContext::new()))
    })
}

/// # Safety
/// `p` comes from `sweet_context_new` and is not used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sweet_context_free(p: *mut SweetContext) {
    if p.is_null() {
        return;
    }
    let c = unsafe { Box::from_raw(p) };
    *c.live.notify.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// Sets the callback run on a worker thread when a refined live conversion
/// is ready.
///
/// # Safety
/// `p` is a live context.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sweet_context_set_notify(
    p: *mut SweetContext,
    f: Option<NotifyFn>,
    user: *mut c_void,
) {
    let c = unsafe { ctx(p) };
    *c.live.notify.lock().unwrap_or_else(|e| e.into_inner()) = f.map(|f| Notify {
        f,
        user: user as usize,
    });
}

/// Handles a key press. `sym` is an X keysym, `unicode` the character it
/// types (0 if none), `mods` a combination of `MOD_*`. Returns `RESULT_*`
/// flags.
///
/// # Safety
/// `p` is a live context.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sweet_process_key(
    p: *mut SweetContext,
    sym: u32,
    unicode: u32,
    mods: u32,
) -> u32 {
    let c = unsafe { ctx(p) };
    guard(0, || c.process_key(sym, unicode, mods))
}

/// # Safety
/// `p` is a live context.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sweet_apply_live(p: *mut SweetContext) -> bool {
    let c = unsafe { ctx(p) };
    guard(false, || c.apply_live())
}

/// Picks entry `index` (absolute) of the candidate list.
///
/// # Safety
/// `p` is a live context.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sweet_select_candidate(p: *mut SweetContext, index: u32) -> bool {
    let c = unsafe { ctx(p) };
    guard(false, || {
        let Some(conv) = converter() else {
            return false;
        };
        let changed = c.session.select_candidate(conv, index as usize);
        c.snapshot();
        changed
    })
}

/// Commits whatever is shown (focus lost, reset).
///
/// # Safety
/// `p` is a live context.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sweet_commit_all(p: *mut SweetContext) {
    let c = unsafe { ctx(p) };
    guard((), || {
        c.session.commit_all();
        c.snapshot();
    })
}

/// # Safety
/// `p` is a live context.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sweet_reset(p: *mut SweetContext) {
    let c = unsafe { ctx(p) };
    guard((), || {
        c.session.reset();
        c.snapshot();
    })
}

/// # Safety
/// `p` is a live context.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sweet_is_active(p: *mut SweetContext) -> bool {
    unsafe { ctx(p) }.session.is_active()
}

/// # Safety
/// `p` is a live context.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sweet_is_open(p: *mut SweetContext) -> bool {
    unsafe { ctx(p) }.open
}

/// # Safety
/// `p` is a live context.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sweet_set_open(p: *mut SweetContext, open: bool) {
    let c = unsafe { ctx(p) };
    guard((), || {
        if !open && c.session.is_active() {
            c.session.commit_all();
        }
        c.open = open;
        c.snapshot();
    })
}

/// Text before the caret in the document (UTF-8), for context-aware
/// conversion.
///
/// # Safety
/// `p` is a live context; `text` is NUL-terminated or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sweet_set_left_context(p: *mut SweetContext, text: *const c_char) {
    let c = unsafe { ctx(p) };
    let t = if text.is_null() {
        "".into()
    } else {
        unsafe { CStr::from_ptr(text) }.to_string_lossy()
    };
    c.session.set_left_context(&t);
}

/// Text to commit (empty if none). Taken by the last state change.
///
/// # Safety
/// `p` is a live context.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sweet_commit_text(p: *mut SweetContext) -> *const c_char {
    unsafe { ctx(p) }.snap.commit.as_ptr()
}

/// # Safety
/// `p` is a live context.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sweet_preedit_text(p: *mut SweetContext) -> *const c_char {
    unsafe { ctx(p) }.snap.preedit.as_ptr()
}

/// Caret position in bytes.
///
/// # Safety
/// `p` is a live context.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sweet_preedit_caret(p: *mut SweetContext) -> u32 {
    unsafe { ctx(p) }.snap.caret
}

/// # Safety
/// `p` is a live context.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sweet_preedit_span_count(p: *mut SweetContext) -> u32 {
    unsafe { ctx(p) }.snap.spans.len() as u32
}

/// Span `i`: byte range and attribute (0 input, 1 converted, 2 focused).
///
/// # Safety
/// `p` is a live context; the out pointers are valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sweet_preedit_span(
    p: *mut SweetContext,
    i: u32,
    start: *mut u32,
    end: *mut u32,
    attr: *mut u32,
) {
    let c = unsafe { ctx(p) };
    let (s, e, a) = c.snap.spans.get(i as usize).copied().unwrap_or_default();
    unsafe {
        *start = s;
        *end = e;
        *attr = a;
    }
}

/// Number of candidates on the shown page (0: no list).
///
/// # Safety
/// `p` is a live context.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sweet_candidate_count(p: *mut SweetContext) -> u32 {
    unsafe { ctx(p) }.snap.candidates.len() as u32
}

/// # Safety
/// `p` is a live context.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sweet_candidate(p: *mut SweetContext, i: u32) -> *const c_char {
    let c = unsafe { ctx(p) };
    c.snap
        .candidates
        .get(i as usize)
        .map_or(c"".as_ptr(), |s| s.as_ptr())
}

/// Selected row on the page, or -1 (suggestions).
///
/// # Safety
/// `p` is a live context.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sweet_candidate_cursor(p: *mut SweetContext) -> i32 {
    unsafe { ctx(p) }.snap.cursor
}

/// Absolute index of the first candidate on the page.
///
/// # Safety
/// `p` is a live context.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sweet_candidate_page_start(p: *mut SweetContext) -> u32 {
    unsafe { ctx(p) }.snap.page_start
}

/// # Safety
/// `p` is a live context.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sweet_candidate_total(p: *mut SweetContext) -> u32 {
    unsafe { ctx(p) }.snap.total
}

/// 0: conversion candidates, 1: predictions (Tab to enter).
///
/// # Safety
/// `p` is a live context.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sweet_candidate_kind(p: *mut SweetContext) -> u32 {
    unsafe { ctx(p) }.snap.kind
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keysyms_go_through_the_keymap() {
        let c = SweetContext::bare();
        // Zenkaku_Hankaku toggles, Henkan turns on (built-in layout).
        assert!(matches!(
            c.translate(0xff2a, 0, 0),
            Some(Input::Toggle(None))
        ));
        assert!(matches!(
            c.translate(0xff23, 0, 0),
            Some(Input::Toggle(Some(true)))
        ));
        assert!(matches!(
            c.translate(0x60, 0x60, MOD_ALT),
            Some(Input::Toggle(None))
        ));
        assert!(matches!(
            c.translate(0xff0d, 0x0d, 0),
            Some(Input::Key(Key::Enter))
        ));
        assert!(matches!(
            c.translate(0x61, 0x61, 0),
            Some(Input::Key(Key::Char('a')))
        ));
        assert!(matches!(
            c.translate(0x41, 0x41, MOD_SHIFT),
            Some(Input::Key(Key::ShiftedLetter('A')))
        ));
        assert!(matches!(
            c.translate(0x2d, 0x2d, 0),
            Some(Input::Key(Key::Char('-')))
        ));
        assert!(c.translate(0x61, 0x61, MOD_CTRL).is_none());
        assert!(matches!(
            c.translate(0xffc4, 0, 0),
            Some(Input::Key(Key::ToKatakana))
        ));
    }

    #[test]
    fn types_and_commits_with_the_dictionary() {
        let dict = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/system.dict");
        if !std::path::Path::new(dict).exists() {
            return;
        }
        unsafe { std::env::set_var("SWEET_IME_DICT", dict) };
        let mut c = SweetContext::new();
        c.session.options.live_conversion = false;
        c.session.options.suggestions = false;
        for ch in "kana".chars() {
            assert_eq!(c.process_key(ch as u32, ch as u32, 0), RESULT_CONSUMED);
        }
        assert_eq!(c.snap.preedit.to_str().unwrap(), "かな");
        assert_eq!(c.process_key(0xff0d, 0x0d, 0), RESULT_CONSUMED);
        assert_eq!(c.snap.commit.to_str().unwrap(), "かな");
        assert_eq!(c.snap.preedit.to_str().unwrap(), "");
        // Off: keys pass through.
        assert_eq!(
            c.process_key(0xff2a, 0, 0),
            RESULT_CONSUMED | RESULT_MODE_CHANGED
        );
        assert_eq!(c.process_key('a' as u32, 'a' as u32, 0), 0);
    }
}
