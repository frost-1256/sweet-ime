//! The TSF text input processor.

use std::cell::RefCell;

use sweet_engine::live::LiveConversionWorker;
use sweet_engine::{Keymap, Session};
use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::TextServices::{
    CLSID_TF_CategoryMgr, GUID_COMPARTMENT_KEYBOARD_OPENCLOSE, IEnumTfDisplayAttributeInfo,
    ITfCategoryMgr, ITfCompartmentEventSink, ITfCompartmentEventSink_Impl, ITfCompartmentMgr,
    ITfComposition, ITfCompositionSink, ITfCompositionSink_Impl, ITfContext, ITfContextView,
    ITfDisplayAttributeInfo, ITfDisplayAttributeProvider, ITfDisplayAttributeProvider_Impl,
    ITfFnConfigure, ITfFnConfigure_Impl, ITfFunction, ITfFunction_Impl, ITfKeyEventSink,
    ITfKeyEventSink_Impl, ITfKeystrokeMgr, ITfLangBarItem, ITfLangBarItemMgr, ITfSource,
    ITfTextInputProcessor, ITfTextInputProcessor_Impl, ITfTextInputProcessorEx,
    ITfTextInputProcessorEx_Impl, ITfTextLayoutSink, ITfTextLayoutSink_Impl, ITfThreadFocusSink,
    ITfThreadFocusSink_Impl, ITfThreadMgr, ITfThreadMgrEx, TF_LC_CHANGE, TF_MOD_ALT,
    TF_MOD_CONTROL, TF_MOD_SHIFT, TF_PRESERVEDKEY, TF_TMF_IMMERSIVEMODE, TfLayoutCode,
};
use windows_core::{BOOL, ComObject, GUID, IUnknownImpl, Interface, Ref, Result, implement};

use crate::candidate_window::{BackgroundResult, CandidateWindow};
use crate::keys::{self, Input, Toggle};
use crate::langbar::ModeButton;
use crate::mode_indicator::ModeIndicator;
use crate::{GUID_ATTR_CONVERTED, GUID_ATTR_FOCUSED, GUID_ATTR_INPUT, display_attr, edit};

mod document;

use document::{document_caret, gui_caret, refresh_candidates, update_document};

static LIVE: LiveConversionWorker = LiveConversionWorker::new();

#[derive(Default)]
struct State {
    thread_mgr: Option<ITfThreadMgr>,
    tid: u32,
    focus_cookie: Option<u32>,
    thread_focused: bool,
    composition: Option<ITfComposition>,
    session: Session,
    /// The current composition started with the preview on in a public field.
    typed_input_allowed: bool,
    window: Option<CandidateWindow>,
    candidate_font: String,
    /// GUID atoms for [input, converted, focused] display attributes.
    atoms: [i32; 3],
    /// Context of the last key event (target for mouse selections).
    ctx: Option<ITfContext>,
    /// Context subscription for positioning candidates after layout.
    layout_sink: Option<(ITfSource, u32)>,
    /// Taskbar input mode indicator.
    mode_button: Option<ComObject<ModeButton>>,
    /// "あ" / "A" badge near the caret.
    indicator: Option<ModeIndicator>,
    keymap: Keymap,
    /// Settings file times when last loaded.
    settings_stamp: [Option<std::time::SystemTime>; 4],
    /// The per-application settings turn the IME off in this application.
    app_off: bool,
    /// Open/close compartment event sink: (compartment source, cookie).
    openclose_sink: Option<(ITfSource, u32)>,
    /// Registered preserved keys for IME mode changes.
    preserved: Vec<(GUID, TF_PRESERVEDKEY, Toggle)>,
    /// Looks taken for the composition's position since it was last found.
    relayout_tries: u32,
}

/// GUID of the `i`-th preserved key.
pub(crate) fn preserved_guid(i: usize) -> GUID {
    GUID::from_u128(0x7e1d3c2a_5b8f_4c1e_9a3d_2f6b8c4e1b00 + i as u128)
}

#[implement(
    ITfTextInputProcessorEx,
    ITfTextInputProcessor,
    ITfKeyEventSink,
    ITfCompositionSink,
    ITfThreadFocusSink,
    ITfTextLayoutSink,
    ITfDisplayAttributeProvider,
    ITfCompartmentEventSink,
    ITfFnConfigure,
    ITfFunction
)]
pub struct TextService {
    st: RefCell<State>,
    /// The candidate window, reachable while `st` is borrowed (layout
    /// changes can arrive during our own edit sessions).
    window_hwnd: std::cell::Cell<isize>,
}

impl TextService {
    pub fn new() -> Self {
        crate::lock_module(true);
        Self { st: RefCell::new(State::default()), window_hwnd: std::cell::Cell::new(0) }
    }
}

impl Drop for TextService {
    fn drop(&mut self) {
        crate::lock_module(false);
    }
}

impl TextService_Impl {
    fn track_context(&self, ctx: &ITfContext) {
        let previous = {
            let mut st = self.st.borrow_mut();
            if st.ctx.as_ref() == Some(ctx) {
                return;
            }
            st.ctx = Some(ctx.clone());
            st.session.cancel_simeji();
            st.layout_sink.take()
        };
        unsafe {
            if let Some((source, cookie)) = previous {
                let _ = source.UnadviseSink(cookie);
            }
            if let Ok(source) = ctx.cast::<ITfSource>() {
                let sink: ITfTextLayoutSink = self.to_interface();
                if let Ok(cookie) = source.AdviseSink(&ITfTextLayoutSink::IID, &sink) {
                    self.st.borrow_mut().layout_sink = Some((source, cookie));
                }
            }
        }
    }

    fn is_open(&self) -> bool {
        let st = self.st.borrow();
        let Some(tm) = &st.thread_mgr else { return false };
        unsafe {
            tm.cast::<ITfCompartmentMgr>()
                .and_then(|m| m.GetCompartment(&GUID_COMPARTMENT_KEYBOARD_OPENCLOSE))
                .and_then(|c| c.GetValue())
                .map(|v| i32::try_from(&v).unwrap_or(0) != 0)
                .unwrap_or(false)
        }
    }

    fn set_open(&self, open: bool) {
        let (tm, tid, button) = {
            let st = self.st.borrow();
            (st.thread_mgr.clone(), st.tid, st.mode_button.clone())
        };
        let Some(tm) = tm else { return };
        // Changing the value calls OnChange (mode badge) synchronously.
        unsafe {
            if let Ok(c) =
                tm.cast::<ITfCompartmentMgr>().and_then(|m| m.GetCompartment(&GUID_COMPARTMENT_KEYBOARD_OPENCLOSE))
            {
                let _ = c.SetValue(tid, &VARIANT::from(open as i32));
            }
        }
        if let Some(b) = button {
            b.set_open(open);
        }
    }

    /// Re-reads options and key bindings if the settings app changed them.
    fn reload_settings_if_changed(&self) {
        let stamp = crate::settings_stamp();
        let Ok(mut st) = self.st.try_borrow_mut() else { return };
        if st.settings_stamp != stamp {
            st.settings_stamp = stamp;
            st.session.options = crate::load_options();
            st.candidate_font = sweet_engine::paths::load_candidate_font();
            st.keymap = sweet_engine::paths::load_keymap();
            st.app_off = crate::app_mode() == sweet_engine::app_modes::AppMode::Off;
            drop(st);
            self.update_preserved_keys();
        }
    }

    /// Registers mode keys that hosts may deliver through OnPreservedKey
    /// instead of the ordinary key event sink.
    fn update_preserved_keys(&self) {
        use sweet_engine::keymap::{Action, preserved_toggles};
        let (tm, tid, old, wanted) = {
            let Ok(mut st) = self.st.try_borrow_mut() else { return };
            (st.thread_mgr.clone(), st.tid, std::mem::take(&mut st.preserved), preserved_toggles(&st.keymap))
        };
        let Some(km) = tm.and_then(|tm| tm.cast::<ITfKeystrokeMgr>().ok()) else { return };
        let mut now = Vec::new();
        unsafe {
            for (g, k, _) in &old {
                let _ = km.UnpreserveKey(g, k);
            }
            for (i, (chord, action)) in wanted.into_iter().enumerate() {
                let mut mods = 0u32;
                if chord.alt {
                    mods |= TF_MOD_ALT;
                }
                if chord.ctrl {
                    mods |= TF_MOD_CONTROL;
                }
                if chord.shift {
                    mods |= TF_MOD_SHIFT;
                }
                let key = TF_PRESERVEDKEY { uVKey: chord.vk as u32, uModifiers: mods };
                let g = preserved_guid(i);
                let desc: Vec<u16> = "IME オン/オフ".encode_utf16().collect();
                match km.PreserveKey(tid, &g, &key, &desc) {
                    Ok(()) => {}
                    Err(e) => crate::debug_log!("PreserveKey {}: {e}", chord.label()),
                }
                // Keep it even if registration failed (unregistered test
                // hosts call OnPreservedKey directly).
                let t = match action {
                    Action::ImeOn => Toggle::On,
                    Action::ImeOff => Toggle::Off,
                    _ => Toggle::Toggle,
                };
                now.push((g, key, t));
            }
        }
        if let Ok(mut st) = self.st.try_borrow_mut() {
            st.preserved = now;
        }
    }

    /// Shows "あ" / "A" at the caret of the focused document.
    fn show_mode_indicator(&self, open: bool) {
        let enabled = self.st.try_borrow().is_ok_and(|st| st.session.options.mode_indicator);
        crate::debug_log!("mode badge: open={open} enabled={enabled}");
        if enabled {
            self.show_badge(if open { "あ" } else { "A" });
        }
    }

    /// Shows a short `text` at the caret of the focused document.
    fn show_badge(&self, text: &'static str) {
        let (tm, tid) = {
            let Ok(st) = self.st.try_borrow() else { return };
            (st.thread_mgr.clone(), st.tid)
        };
        let Some(tm) = tm else { return };
        let ctx = unsafe { tm.GetFocus().and_then(|d| d.GetTop()).ok() };
        crate::debug_log!("mode badge: focus context={}", ctx.is_some());
        let this: ComObject<TextService> = self.to_object();
        let Some(ctx) = ctx else {
            if let Some((rc, owner)) = gui_caret() {
                this.indicator_at(text, rc, owner);
            }
            return;
        };
        let ctx2 = ctx.clone();
        let r = edit::request_any(&ctx, tid, move |ec| {
            let doc = unsafe { document_caret(ec, &ctx2) };
            crate::debug_log!("mode badge: document caret {doc:?}");
            let found = doc.or_else(gui_caret);
            crate::debug_log!("mode badge: caret {found:?}");
            if let Some((rc, owner)) = found {
                this.indicator_at(text, rc, owner);
            }
            Ok(())
        });
        crate::debug_log!("mode badge: edit session {r:?}");
    }

    fn indicator_at(&self, text: &str, caret: RECT, owner: Option<HWND>) {
        let Ok(mut st) = self.st.try_borrow_mut() else {
            crate::debug_log!("mode badge: state busy");
            return;
        };
        if st.indicator.is_none() {
            st.indicator = ModeIndicator::new().inspect_err(|e| crate::debug_log!("mode badge: window {e}")).ok();
        }
        if let Some(i) = &st.indicator {
            i.show(text, caret, owner);
        }
    }

    /// Stores a reported conversion and says so at the caret.
    fn save_report(&self, report: sweet_engine::reports::Report) {
        let saved = sweet_engine::paths::reports_file()
            .ok_or_else(|| std::io::Error::other("no settings directory"))
            .and_then(|p| sweet_engine::reports::add(&p, report));
        crate::debug_log!("report saved: {saved:?}");
        self.show_badge(if saved.is_ok() { "報告済み" } else { "報告できません" });
    }

    /// The taskbar mode indicator was clicked.
    fn on_mode_click(&self) {
        let _ = crate::guard(|| {
            self.handle_toggle(None, Toggle::Toggle);
            Ok(())
        });
    }

    fn translate(&self, vk: u32, lparam: isize) -> Option<Input> {
        let st = self.st.borrow();
        if st.app_off {
            return None;
        }
        keys::translate(&st.keymap, vk, lparam, st.session.is_active())
    }

    /// Whether the key-down event would be consumed.
    fn wants_key(&self, vk: u32, lparam: isize) -> bool {
        let key = match self.translate(vk, lparam) {
            None => return false,
            Some(Input::Toggle(_)) => return true,
            Some(Input::Key(k)) => k,
        };
        self.allows_key(key)
    }

    fn allows_key(&self, key: sweet_engine::Key) -> bool {
        if crate::converter().is_none() || !self.is_open() {
            return false;
        }
        if self.st.borrow().session.is_active() {
            return true;
        }
        if key == sweet_engine::Key::UndoCommit {
            // Otherwise the application keeps the key (Ctrl+Backspace
            // deletes a word).
            return self.st.borrow().session.can_undo();
        }
        if key == sweet_engine::Key::ReportCommit {
            return self.st.borrow().session.reportable_commit().is_some();
        }
        matches!(
            key,
            sweet_engine::Key::Char(_)
                | sweet_engine::Key::ShiftedLetter(_)
                | sweet_engine::Key::Space
        )
    }

    /// Starts refining the live conversion in the background (reranker only).
    fn request_live_refinement(&self) {
        let Some(conv) = crate::converter() else { return };
        if !conv.has_reranker() {
            return;
        }
        let st = self.st.borrow();
        let (Some((reading, left_context)), Some(w)) = (st.session.live_request(), &st.window)
        else {
            return;
        };
        let hwnd = w.raw();
        LIVE.submit(conv, reading, left_context, move |reading, surface| {
            crate::candidate_window::post_live(hwnd, reading, surface);
        });
    }

    /// A background live conversion result arrived (UI thread).
    fn on_live_result(&self, result: BackgroundResult) {
        let _ = crate::guard(|| {
            let Ok(mut st) = self.st.try_borrow_mut() else { return Ok(()) };
            let changed = match result {
                BackgroundResult::Live(reading, surface) => st.session.apply_live(&reading, &surface),
                BackgroundResult::Simeji(response) => {
                    st.thread_focused
                        && crate::converter().is_some_and(|conv| st.session.apply_simeji(conv, response))
                }
            };
            if !changed {
                return Ok(());
            }
            let (ctx, tid) = (st.ctx.clone(), st.tid);
            drop(st);
            if let Some(ctx) = ctx {
                let this: ComObject<TextService> = self.to_object();
                let ctx2 = ctx.clone();
                edit::request_any(&ctx, tid, move |ec| update_document(&this, ec, &ctx2))?;
            }
            Ok(())
        });
    }

    /// Positions the candidate list again; `retry` when no layout change
    /// prompted it (the composition had no position yet).
    fn relayout(&self, retry: bool) {
        let _ = crate::guard(|| {
            let Ok(st) = self.st.try_borrow() else { return Ok(()) };
            if !st.thread_focused
                || st.composition.is_none()
                || ((!st.session.options.show_typed_input || !st.typed_input_allowed)
                    && st.session.candidates().is_none())
            {
                return Ok(());
            }
            let (Some(target), tid) = (st.ctx.clone(), st.tid) else { return Ok(()) };
            drop(st);
            let this: ComObject<TextService> = self.to_object();
            let ctx = target.clone();
            edit::request_layout(&ctx, tid, move |ec| {
                if let Ok(mut st) = this.st.try_borrow_mut() {
                    let current = st.thread_mgr.as_ref().and_then(|tm| unsafe {
                        tm.GetFocus().and_then(|doc| doc.GetTop()).ok()
                    });
                    if st.thread_focused && current.as_ref() == Some(&target) && st.ctx.as_ref() == Some(&target) {
                        refresh_candidates(&this, &mut st, ec, &target, retry)?;
                    }
                }
                Ok(())
            })
        });
    }

    /// A candidate row was clicked.
    fn on_candidate_click(&self, index: usize) {
        let _ = crate::guard(|| {
            let Some(conv) = crate::converter() else { return Ok(()) };
            let Ok(mut st) = self.st.try_borrow_mut() else { return Ok(()) };
            let changed = st.session.select_candidate(conv, index);
            let ctx = st.ctx.clone();
            let tid = st.tid;
            drop(st);
            if let (true, Some(ctx)) = (changed, ctx) {
                let this: ComObject<TextService> = self.to_object();
                let ctx2 = ctx.clone();
                edit::request_any(&ctx, tid, move |ec| update_document(&this, ec, &ctx2))?;
            }
            Ok(())
        });
    }

    /// Pushes the session state (commit text, preedit, candidates) into the
    /// document through an edit session.
    fn update(&self, ctx: &ITfContext, cloud_reading: Option<String>) -> Result<()> {
        let this: ComObject<TextService> = self.to_object();
        let tid = self.st.borrow().tid;
        let target = ctx.clone();
        edit::request(ctx, tid, move |ec| {
            update_document(&this, ec, &target)?;
            if let Some(reading) = &cloud_reading
                && document::private_field(&target, ec).ok() == Some(false)
            {
                let mut st = this.st.borrow_mut();
                if let Some(hwnd) = st.window.as_ref().map(CandidateWindow::raw) {
                    st.session.request_simeji(reading.clone(), move |response| {
                        crate::candidate_window::post_result(hwnd, BackgroundResult::Simeji(response));
                    });
                }
            }
            Ok(())
        })
    }

    fn handle_toggle(&self, ctx: Option<&ITfContext>, t: Toggle) {
        let open = self.is_open();
        let new = match t {
            Toggle::Toggle => !open,
            Toggle::On => true,
            Toggle::Off => false,
        };
        if !new {
            let (pending, tid) = {
                let st = self.st.borrow();
                let ctx = if st.session.is_active() { st.ctx.clone().or_else(|| ctx.cloned()) } else { None };
                (ctx.map(|ctx| (ctx, st.session.preedit())), st.tid)
            };
            if let Some((ctx, preedit)) = pending {
                let this: ComObject<TextService> = self.to_object();
                let destination = ctx.clone();
                // Some hosts only grant an asynchronous edit lock here.
                let result = edit::request_any(&ctx, tid, move |ec| {
                    let mut st = this.st.borrow_mut();
                    if st.ctx.as_ref().is_some_and(|current| current != &destination) || st.session.preedit() != preedit {
                        return Ok(());
                    }
                    st.session.commit_all();
                    drop(st);
                    update_document(&this, ec, &destination)
                });
                if let Err(e) = result {
                    crate::debug_log!("finish composition on IME off: {e}");
                }
            }
        }
        if new != open {
            self.set_open(new);
        }
    }
}

/// Creates the candidate window with its handlers. The closures keep the
/// service alive; Deactivate drops the window, which breaks the cycle.
fn make_window(this: &ComObject<TextService>, owner: Option<HWND>) -> Option<CandidateWindow> {
    let w = CandidateWindow::new(owner).ok()?;
    w.warm_up();
    let t = this.clone();
    w.set_on_click(move |i| t.on_candidate_click(i));
    let t = this.clone();
    w.set_on_live(move |result| t.on_live_result(result));
    let t = this.clone();
    w.set_on_relayout(move || t.relayout(true));
    this.window_hwnd.set(w.raw());
    Some(w)
}

/// Whether the thread hosts an immersive (UWP / shell) application.
fn immersive(tm: &ITfThreadMgr) -> bool {
    unsafe {
        tm.cast::<ITfThreadMgrEx>()
            .and_then(|t| t.GetActiveFlags())
            .is_ok_and(|f| f & TF_TMF_IMMERSIVEMODE != 0)
    }
}

impl ITfTextInputProcessor_Impl for TextService_Impl {
    fn Activate(&self, ptim: Ref<ITfThreadMgr>, tid: u32) -> Result<()> {
        self.ActivateEx(ptim, tid, 0)
    }

    fn Deactivate(&self) -> Result<()> {
        crate::guard(|| {
            let mut st = self.st.borrow_mut();
            if let Some(button) = st.mode_button.take() {
                if let Some(mgr) = st.thread_mgr.as_ref().and_then(|tm| tm.cast::<ITfLangBarItemMgr>().ok()) {
                    let item: ITfLangBarItem = button.to_interface();
                    unsafe {
                        let _ = mgr.RemoveItem(&item);
                    }
                }
                button.detach();
            }
            if let Some(tm) = st.thread_mgr.take() {
                unsafe {
                    if let Ok(km) = tm.cast::<ITfKeystrokeMgr>() {
                        for (g, k, _) in st.preserved.drain(..) {
                            let _ = km.UnpreserveKey(&g, &k);
                        }
                        let _ = km.UnadviseKeyEventSink(st.tid);
                    }
                    if let (Some(cookie), Ok(src)) = (st.focus_cookie.take(), tm.cast::<ITfSource>()) {
                        let _ = src.UnadviseSink(cookie);
                    }
                }
            }
            if let Some((source, cookie)) = st.layout_sink.take() {
                unsafe { let _ = source.UnadviseSink(cookie); }
            }
            if let Some((src, cookie)) = st.openclose_sink.take() {
                unsafe {
                    let _ = src.UnadviseSink(cookie);
                }
            }
            st.composition = None;
            crate::set_busy(false);
            st.session.reset();
            st.window = None;
            st.indicator = None;
            st.ctx = None;
            Ok(())
        })
    }
}

impl ITfTextInputProcessorEx_Impl for TextService_Impl {
    fn ActivateEx(&self, ptim: Ref<ITfThreadMgr>, tid: u32, flags: u32) -> Result<()> {
        let hot_swap = flags & crate::ACTIVATE_HOT_SWAP != 0;
        use sweet_engine::app_modes::AppMode;
        let app_mode = crate::app_mode();
        crate::guard(|| {
            let tm = ptim.ok()?.clone();
            {
                let mut st = self.st.borrow_mut();
                st.thread_mgr = Some(tm.clone());
                st.tid = tid;
                st.thread_focused = true;
            }
            unsafe {
                let km: ITfKeystrokeMgr = tm.cast()?;
                let sink: ITfKeyEventSink = self.to_interface();
                // Fails for unregistered CLSIDs (test harness); keep going.
                if let Err(e) = km.AdviseKeyEventSink(tid, &sink, true) {
                    crate::debug_log!("AdviseKeyEventSink: {e}");
                }
                let src: ITfSource = tm.cast()?;
                let fs: ITfThreadFocusSink = self.to_interface();
                let cookie = src
                    .AdviseSink(&ITfThreadFocusSink::IID, &fs)
                    .inspect_err(|e| crate::debug_log!("AdviseSink(ThreadFocus): {e}"))?;
                self.st.borrow_mut().focus_cookie = Some(cookie);
                let cats: ITfCategoryMgr =
                    CoCreateInstance(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER)?;
                let mut atoms = [0i32; 3];
                for (i, g) in [GUID_ATTR_INPUT, GUID_ATTR_CONVERTED, GUID_ATTR_FOCUSED].iter().enumerate() {
                    atoms[i] = cats
                        .RegisterGUID(g)
                        .inspect_err(|e| crate::debug_log!("RegisterGUID: {e}"))? as i32;
                }
                self.st.borrow_mut().atoms = atoms;
            }
            // Start in hiragana mode; also warms up the dictionary mapping.
            let _ = crate::converter();
            // Create the candidate window now so the first keystroke that
            // shows suggestions does not pay for Direct2D initialization.
            // In immersive hosts it is created on first use instead, owned
            // by the host window from the start.
            {
                let this: ComObject<TextService> = self.to_object();
                let mut st = self.st.borrow_mut();
                st.session.options = crate::load_options();
                st.candidate_font = sweet_engine::paths::load_candidate_font();
                st.keymap = sweet_engine::paths::load_keymap();
                st.settings_stamp = crate::settings_stamp();
                st.app_off = app_mode == AppMode::Off;
                let imm = st.thread_mgr.as_ref().is_some_and(immersive);
                crate::debug_log!("activate: immersive={imm}");
                if st.window.is_none() && !imm {
                    st.window = make_window(&this, None);
                }
            }
            // Taskbar indicator ("あ" / "A"). The handler holds the service;
            // Deactivate detaches it.
            {
                let this: ComObject<TextService> = self.to_object();
                let button = ComObject::new(ModeButton::new(true, move || this.on_mode_click()));
                let tm = self.st.borrow().thread_mgr.clone();
                if let Some(mgr) = tm.and_then(|tm| tm.cast::<ITfLangBarItemMgr>().ok()) {
                    let item: ITfLangBarItem = button.to_interface();
                    match unsafe { mgr.AddItem(&item) } {
                        Ok(()) => self.st.borrow_mut().mode_button = Some(button),
                        Err(e) => {
                            button.detach();
                            crate::debug_log!("AddItem: {e}");
                        }
                    }
                } else {
                    button.detach();
                }
            }
            if hot_swap {
                // Replacing an older core: keep the user's input mode.
                if let Some(b) = self.st.borrow().mode_button.clone() {
                    b.set_open(self.is_open());
                }
            } else {
                // Hiragana, unless this application starts in direct input.
                self.set_open(app_mode == AppMode::Normal);
            }
            // Watch the open/close state (changed by keys, the taskbar or
            // other components) to show the mode badge.
            unsafe {
                let tm = self.st.borrow().thread_mgr.clone();
                let src = tm
                    .and_then(|tm| tm.cast::<ITfCompartmentMgr>().ok())
                    .and_then(|m| m.GetCompartment(&GUID_COMPARTMENT_KEYBOARD_OPENCLOSE).ok())
                    .and_then(|c| c.cast::<ITfSource>().ok());
                if let Some(src) = src {
                    let sink: ITfCompartmentEventSink = self.to_interface();
                    match src.AdviseSink(&ITfCompartmentEventSink::IID, &sink) {
                        Ok(cookie) => self.st.borrow_mut().openclose_sink = Some((src, cookie)),
                        Err(e) => crate::debug_log!("AdviseSink(compartment): {e}"),
                    }
                }
            }
            self.update_preserved_keys();
            // Switched to Sweet IME (e.g. Win+Space): show the mode.
            if !hot_swap && app_mode != AppMode::Off {
                self.show_mode_indicator(self.is_open());
            }
            Ok(())
        })
    }
}

impl ITfCompartmentEventSink_Impl for TextService_Impl {
    fn OnChange(&self, rguid: *const GUID) -> Result<()> {
        if rguid.is_null() || unsafe { *rguid } != GUID_COMPARTMENT_KEYBOARD_OPENCLOSE {
            return Ok(());
        }
        let _ = crate::guard(|| {
            let open = self.is_open();
            let button = self.st.try_borrow().ok().and_then(|st| st.mode_button.clone());
            if let Some(b) = button {
                b.set_open(open);
            }
            self.show_mode_indicator(open);
            Ok(())
        });
        Ok(())
    }
}

impl ITfKeyEventSink_Impl for TextService_Impl {
    fn OnSetFocus(&self, _foreground: BOOL) -> Result<()> {
        Ok(())
    }

    fn OnTestKeyDown(&self, _pic: Ref<ITfContext>, wparam: WPARAM, lparam: LPARAM) -> Result<BOOL> {
        crate::guard(|| {
            let wants = self.wants_key(wparam.0 as u32, lparam.0);
            crate::debug_log!("test key 0x{:02X} -> {wants}", wparam.0);
            Ok(BOOL(wants as i32))
        })
    }

    fn OnTestKeyUp(&self, _pic: Ref<ITfContext>, _wparam: WPARAM, _lparam: LPARAM) -> Result<BOOL> {
        Ok(BOOL(0))
    }

    fn OnKeyDown(&self, pic: Ref<ITfContext>, wparam: WPARAM, lparam: LPARAM) -> Result<BOOL> {
        crate::guard(|| {
            let vk = wparam.0 as u32;
            crate::debug_log!("key down 0x{vk:02X}");
            let key = match self.translate(vk, lparam.0) {
                None => return Ok(BOOL(0)),
                Some(Input::Toggle(t)) => {
                    self.handle_toggle(pic.as_ref(), t);
                    return Ok(BOOL(1));
                }
                Some(Input::Key(k)) => k,
            };
            if !self.allows_key(key) {
                return Ok(BOOL(0));
            }
            let Some(conv) = crate::converter() else { return Ok(BOOL(0)) };
            let ctx = pic.ok()?.clone();
            self.track_context(&ctx);
            let cloud_reading = self.st.borrow().session.simeji_conversion_reading(key);
            let consumed = self.st.borrow_mut().session.handle(conv, key);
            let report = self.st.borrow_mut().session.take_report();
            if let Some(report) = report {
                self.save_report(report);
                return Ok(BOOL(1));
            }
            if consumed {
                if let Some(i) = &self.st.borrow().indicator {
                    i.hide();
                }
                self.update(&ctx, cloud_reading)?;
                self.request_live_refinement();
            }
            Ok(BOOL(consumed as i32))
        })
    }

    fn OnKeyUp(&self, _pic: Ref<ITfContext>, _wparam: WPARAM, _lparam: LPARAM) -> Result<BOOL> {
        Ok(BOOL(0))
    }

    fn OnPreservedKey(&self, pic: Ref<ITfContext>, rguid: *const GUID) -> Result<BOOL> {
        crate::debug_log!("OnPreservedKey");
        crate::guard(|| {
            if rguid.is_null() {
                return Ok(BOOL(0));
            }
            let g = unsafe { *rguid };
            crate::debug_log!("preserved key {g:?}");
            let st = self.st.borrow();
            let t = st.preserved.iter().find(|(x, _, _)| *x == g).filter(|_| !st.app_off).map(|(_, _, t)| *t);
            drop(st);
            let Some(t) = t else { return Ok(BOOL(0)) };
            self.handle_toggle(pic.as_ref(), t);
            Ok(BOOL(1))
        })
    }
}

impl ITfCompositionSink_Impl for TextService_Impl {
    fn OnCompositionTerminated(&self, _ec: u32, _comp: Ref<ITfComposition>) -> Result<()> {
        // The application ended the composition (e.g. clicked elsewhere).
        // The text stays in the document as-is.
        let _ = crate::guard(|| {
            if let Ok(mut st) = self.st.try_borrow_mut() {
                st.composition = None;
                crate::set_busy(false);
                st.session.reset();
                if let Some(w) = &st.window {
                    w.hide();
                }
            }
            Ok(())
        });
        Ok(())
    }
}

impl ITfTextLayoutSink_Impl for TextService_Impl {
    fn OnLayoutChange(&self, pic: Ref<ITfContext>, code: TfLayoutCode, _view: Ref<ITfContextView>) -> Result<()> {
        crate::guard(|| {
            if pic.as_ref().is_none() || code != TF_LC_CHANGE {
                return Ok(());
            }
            let Ok(st) = self.st.try_borrow() else {
                // Laid out during our own edit session: look once it is over,
                // or the list may wait for the next key.
                let hwnd = self.window_hwnd.get();
                if hwnd != 0 {
                    crate::candidate_window::relayout_after(hwnd, 10);
                }
                return Ok(());
            };
            if st.ctx.as_ref() != pic.as_ref() {
                return Ok(());
            }
            drop(st);
            self.relayout(false);
            Ok(())
        })
    }
}

impl ITfThreadFocusSink_Impl for TextService_Impl {
    fn OnSetThreadFocus(&self) -> Result<()> {
        let _ = crate::guard(|| {
            if let Ok(mut st) = self.st.try_borrow_mut() {
                st.thread_focused = true;
            }
            self.reload_settings_if_changed();
            Ok(())
        });
        Ok(())
    }

    fn OnKillThreadFocus(&self) -> Result<()> {
        let _ = crate::guard(|| {
            if let Ok(mut st) = self.st.try_borrow_mut() {
                st.thread_focused = false;
                st.session.cancel_simeji();
                if let Some(w) = &st.window {
                    w.hide();
                }
                if let Some(i) = &st.indicator {
                    i.hide();
                }
            }
            Ok(())
        });
        Ok(())
    }
}

/// "Options" in Windows language settings.
impl ITfFunction_Impl for TextService_Impl {
    fn GetDisplayName(&self) -> Result<windows_core::BSTR> {
        Ok(windows_core::BSTR::from("Sweet IME の設定"))
    }
}

impl ITfFnConfigure_Impl for TextService_Impl {
    fn Show(&self, _parent: HWND, _langid: u16, _profile: *const GUID) -> Result<()> {
        crate::launch_settings();
        Ok(())
    }
}

impl ITfDisplayAttributeProvider_Impl for TextService_Impl {
    fn EnumDisplayAttributeInfo(&self) -> Result<IEnumTfDisplayAttributeInfo> {
        Ok(display_attr::EnumInfo::new())
    }

    fn GetDisplayAttributeInfo(&self, guid: *const GUID) -> Result<ITfDisplayAttributeInfo> {
        if guid.is_null() {
            return Err(windows::Win32::Foundation::E_INVALIDARG.into());
        }
        display_attr::info(unsafe { &*guid })
    }
}
