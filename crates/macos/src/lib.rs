//! InputMethodKit bridge. The controller owns a context and calls it on the
//! main thread. Only the shared engine's live conversion runs in the worker.

use std::ffi::{CStr, CString, c_char};
use std::sync::OnceLock;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use serde_json::json;
use sweet_engine::keymap::{Action, Chord, Keymap, ModifierTap};
use sweet_engine::live::LiveConversionWorker;
use sweet_engine::simeji;
use sweet_engine::{CandidateList, Converter, Dictionary, Key, ListKind, Session, SpanAttr, paths};

static CONVERTER: OnceLock<Converter> = OnceLock::new();
static LIVE: LiveConversionWorker = LiveConversionWorker::new();

pub struct Context {
    converter: &'static Converter,
    session: Session,
    keymap: Keymap,
    modifier_tap: ModifierTap,
    enabled: bool,
    live: Option<Receiver<(String, String)>>,
    cloud: Option<Receiver<simeji::Response>>,
    snapshot: CString,
}

impl Context {
    fn reload(&mut self) {
        self.modifier_tap = ModifierTap::default();
        let options = paths::load_options();
        self.converter.apply_options(&options);
        self.session.options = options;
        self.keymap = paths::load_keymap();
        paths::apply_romaji_table();
    }

    fn handle(&mut self, vk: u16, character: u32, modifiers: u8) -> bool {
        let chord = Chord {
            vk,
            shift: modifiers & 1 != 0,
            ctrl: modifiers & 2 != 0,
            alt: modifiers & 4 != 0,
        };
        let action = self.keymap.lookup(chord, self.session.is_active());
        let key = match action {
            Some(Action::ImeOn) => {
                self.enabled = true;
                return true;
            }
            Some(Action::ImeToggle | Action::ImeOff) => {
                self.enabled = action == Some(Action::ImeToggle) && !self.enabled;
                if !self.enabled {
                    self.finish(true);
                }
                return true;
            }
            Some(Action::Key(key)) => key,
            None if !chord.ctrl && !chord.alt => match char::from_u32(character) {
                Some(ch) if ch.is_ascii_graphic() => {
                    if chord.shift && ch.is_ascii_alphabetic() {
                        Key::ShiftedLetter(ch)
                    } else {
                        Key::Char(ch)
                    }
                }
                _ => return false,
            },
            None => return false,
        };
        let cloud_reading = self.session.simeji_conversion_reading(key);
        if !self.enabled
            || key == Key::UndoCommit
            || !(self.session.is_active()
                || matches!(
                    key,
                    Key::Char(_)
                        | Key::ShiftedLetter(_)
                        | Key::Space
                        | Key::HalfSpace
                        | Key::ReportCommit
                ))
            || !self.session.handle(self.converter, key)
        {
            return false;
        }
        if let Some(reading) = cloud_reading {
            let (send, receive) = mpsc::channel();
            if self.session.request_simeji(reading, move |response| {
                let _ = send.send(response);
            }) {
                self.cloud = Some(receive);
            }
        }
        if let Some(report) = self.session.take_report()
            && let Some(path) = paths::reports_file()
        {
            let _ = sweet_engine::reports::add(&path, report);
        }
        if self.session.is_converting() && key != Key::Escape {
            self.session.open_candidates(self.converter);
        }
        self.request_live();
        true
    }

    fn request_live(&mut self) {
        self.live = None;
        if !self.session.options.neural || !self.converter.has_reranker() {
            return;
        }
        if let Some((reading, left_context)) = self.session.live_request() {
            let (send, receive) = mpsc::channel();
            let debounce =
                std::time::Duration::from_millis(self.session.options.live_debounce_ms.into());
            LIVE.submit_with_debounce(
                self.converter,
                reading,
                left_context,
                debounce,
                move |reading, surface| {
                    let _ = send.send((reading, surface));
                },
            );
            self.live = Some(receive);
        }
    }

    fn page(&self) -> Option<CandidateList> {
        self.session
            .candidates()
            .filter(|list| !list.items.is_empty())
    }

    fn select(&mut self, row: usize) -> bool {
        let Some(list) = self.page() else {
            return false;
        };
        if row >= list.page_size
            || !self
                .session
                .select_candidate(self.converter, list.page_start() + row)
        {
            return false;
        }
        self.live = None;
        if self.session.is_converting() {
            self.session.open_candidates(self.converter);
        }
        true
    }

    fn finish(&mut self, commit: bool) {
        self.modifier_tap = ModifierTap::default();
        self.live = None;
        self.cloud = None;
        if commit {
            self.session.commit_all();
        } else {
            self.session.reset();
            self.session.take_commit();
        }
    }

    fn snapshot(&mut self) -> &CStr {
        let preedit = self.session.preedit();
        let utf16_index = |index| preedit.text[..index].encode_utf16().count();
        let spans: Vec<_> = preedit
            .spans
            .iter()
            .map(|&(start, end, attr)| {
                (
                    utf16_index(start),
                    utf16_index(end),
                    attr == SpanAttr::Focused,
                )
            })
            .collect();
        let mut selected = None;
        let list = self.page();
        let candidates = list.as_ref().map_or(&[][..], |list| {
            let start = list.page_start();
            if list.kind == ListKind::Conversion {
                selected = Some(list.selected - start);
            }
            &list.items[start..(start + list.page_size).min(list.items.len())]
        });
        let value = json!({
            "commit": self.session.take_commit(),
            "preedit": preedit.text,
            "caret": utf16_index(preedit.caret),
            "spans": spans,
            "candidates": candidates,
            "selected": selected,
        });
        // JSON serialization escapes control characters, including NUL.
        self.snapshot = CString::new(value.to_string()).expect("JSON is NUL-free");
        &self.snapshot
    }
}

/// C entry points are declared in macos/bridge.h. The controller guarantees
/// valid pointers and takes a copy of each snapshot before calling again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sweet_macos_load(dictionary: *const c_char) -> bool {
    if CONVERTER.get().is_some() {
        return true;
    }
    let path = unsafe { CStr::from_ptr(dictionary) };
    let Ok(path) = path.to_str() else {
        return false;
    };
    let Ok(dictionary) = Dictionary::open(path) else {
        return false;
    };
    let converter = paths::with_user_data(Converter::new(dictionary));
    let mut models = std::env::var_os("SWEET_IME_ZENZ")
        .map(std::path::PathBuf::from)
        .into_iter()
        .chain(paths::zenz_file());
    let model = models.find_map(|path| sweet_zenz::Zenz::load(&path).ok());
    let converter = match model {
        Some(model) => converter.with_reranker(Box::new(sweet_zenz::ZenzReranker::new(model))),
        None => converter,
    };
    let _ = CONVERTER.set(converter);
    true
}

#[unsafe(no_mangle)]
pub extern "C" fn sweet_macos_new() -> *mut Context {
    let Some(converter) = CONVERTER.get() else {
        return std::ptr::null_mut();
    };
    let mut context = Context {
        converter,
        session: Session::new(),
        keymap: Keymap::default(),
        modifier_tap: ModifierTap::default(),
        enabled: true,
        live: None,
        cloud: None,
        snapshot: CString::default(),
    };
    context.reload();
    Box::into_raw(Box::new(context))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sweet_macos_free(context: *mut Context) {
    drop(unsafe { Box::from_raw(context) });
}

#[unsafe(no_mangle)]
pub extern "C" fn sweet_macos_activate(context: &mut Context) {
    context.reload();
}

#[unsafe(no_mangle)]
pub extern "C" fn sweet_macos_key(
    context: &mut Context,
    vk: u16,
    character: u32,
    modifiers: u8,
) -> bool {
    context.modifier_tap.press(0, false);
    context.handle(vk, character, modifiers)
}

#[unsafe(no_mangle)]
pub extern "C" fn sweet_macos_modifier(
    context: &mut Context,
    vk: u16,
    pressed: bool,
    alone: bool,
) -> bool {
    if pressed {
        context.modifier_tap.press(vk, alone);
        false
    } else {
        context.modifier_tap.release(vk) && alone && context.handle(vk, 0, 0)
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn sweet_macos_select(context: &mut Context, row: usize) -> bool {
    context.select(row)
}

#[unsafe(no_mangle)]
pub extern "C" fn sweet_macos_finish(context: &mut Context, commit: bool) {
    context.finish(commit);
}

#[unsafe(no_mangle)]
pub extern "C" fn sweet_macos_snapshot(context: &mut Context) -> *const c_char {
    context.snapshot().as_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn sweet_macos_live_pending(context: &Context) -> bool {
    context.live.is_some() || context.cloud.is_some()
}

#[unsafe(no_mangle)]
pub extern "C" fn sweet_macos_apply_live(context: &mut Context) -> bool {
    if let Some(result) = context.cloud.as_ref().map(Receiver::try_recv) {
        match result {
            Ok(response) => {
                context.cloud = None;
                if context.session.apply_simeji(context.converter, response) {
                    context.live = None;
                    return true;
                }
            }
            Err(TryRecvError::Disconnected) => {
                context.cloud = None;
                context.session.cancel_simeji();
            }
            Err(TryRecvError::Empty) => {}
        }
    }
    match context.live.as_ref().map(Receiver::try_recv) {
        Some(Ok((reading, surface))) => {
            context.live = None;
            context.session.apply_live(&reading, &surface)
        }
        Some(Err(TryRecvError::Disconnected)) => {
            context.live = None;
            false
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_and_commits_a_candidate_through_the_bridge() {
        let dictionary =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/system.dict");
        if !dictionary.is_file() {
            return; // Same dictionary requirement as the engine's session tests.
        }
        let converter = Box::leak(Box::new(Converter::new(
            Dictionary::open(dictionary).unwrap(),
        )));
        let mut context = Context {
            converter,
            session: Session::new(),
            keymap: Keymap::default(),
            modifier_tap: ModifierTap::default(),
            enabled: true,
            live: None,
            cloud: None,
            snapshot: CString::default(),
        };
        context
            .keymap
            .bindings
            .extend(Keymap::parse("#! version 3\nLeftAlt ime_off\nRightAlt ime_on\n").bindings);
        assert!(!sweet_macos_modifier(&mut context, 0xA4, true, true));
        assert!(sweet_macos_modifier(&mut context, 0xA4, false, true));
        assert!(!context.enabled);
        assert!(!sweet_macos_modifier(&mut context, 0xA5, true, true));
        assert!(!sweet_macos_key(&mut context, 0x41, 'a' as u32, 4));
        assert!(!sweet_macos_modifier(&mut context, 0xA5, false, true));
        assert!(!context.enabled);
        assert!(!sweet_macos_modifier(&mut context, 0xA5, true, true));
        assert!(sweet_macos_modifier(&mut context, 0xA5, false, true));
        assert!(context.enabled);

        context.session.options.live_conversion = false;
        assert!(context.handle(0x20, ' ' as u32, 0));
        let state: serde_json::Value =
            serde_json::from_slice(context.snapshot().to_bytes()).unwrap();
        assert_eq!(state["commit"], "　");
        for ch in "kana".chars() {
            assert!(context.handle(ch.to_ascii_uppercase() as u16, ch as u32, 0));
        }
        let state: serde_json::Value =
            serde_json::from_slice(context.snapshot().to_bytes()).unwrap();
        assert_eq!(state["preedit"], "かな");
        assert_eq!(state["caret"], 2);
        assert_eq!(state["spans"], json!([[0, 2, false]]));
        assert!(context.handle(0x20, ' ' as u32, 0));
        let state: serde_json::Value =
            serde_json::from_slice(context.snapshot().to_bytes()).unwrap();
        let row = state["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .position(|item| item == "仮名")
            .unwrap();
        assert!(context.select(row));
        let state: serde_json::Value =
            serde_json::from_slice(context.snapshot().to_bytes()).unwrap();
        assert_eq!(state["commit"], "仮名");
        assert_eq!(state["preedit"], "");
    }
}
