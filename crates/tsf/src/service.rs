//! The TSF text input processor.

use std::cell::RefCell;

use sweet_engine::{Keymap, Session, SpanAttr};
use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::GetFocus;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::TextServices::{
    CLSID_TF_CategoryMgr, GUID_COMPARTMENT_KEYBOARD_OPENCLOSE, GUID_PROP_ATTRIBUTE,
    IEnumTfDisplayAttributeInfo, ITfCategoryMgr, ITfCompartmentMgr, ITfComposition,
    ITfCompositionSink, ITfCompositionSink_Impl, ITfContext, ITfContextComposition,
    ITfDisplayAttributeInfo, ITfDisplayAttributeProvider, ITfDisplayAttributeProvider_Impl,
    ITfCompartmentEventSink, ITfCompartmentEventSink_Impl, ITfInsertAtSelection, ITfKeyEventSink, ITfLangBarItem, ITfLangBarItemMgr, ITfKeyEventSink_Impl, ITfKeystrokeMgr, ITfRange,
    ITfSource, ITfTextInputProcessor, ITfTextInputProcessor_Impl, ITfTextInputProcessorEx,
    ITfTextInputProcessorEx_Impl, ITfThreadFocusSink, ITfThreadFocusSink_Impl, ITfThreadMgr, ITfThreadMgrEx, ITfFnConfigure, ITfFnConfigure_Impl, ITfFunction, ITfFunction_Impl, TF_TMF_IMMERSIVEMODE,
    TF_AE_NONE, TF_ANCHOR_END, TF_DEFAULT_SELECTION, TF_ANCHOR_START, TF_IAS_QUERYONLY, TF_SELECTION,
    TF_SELECTIONSTYLE,
};
use windows_core::{BOOL, ComObject, GUID, IUnknownImpl, Interface, Ref, Result, implement};

use crate::candidate_window::CandidateWindow;
use crate::keys::{self, Input, Toggle};
use crate::mode_indicator::ModeIndicator;
use crate::langbar::ModeButton;
use crate::{GUID_ATTR_CONVERTED, GUID_ATTR_FOCUSED, GUID_ATTR_INPUT, display_attr, edit};

#[derive(Default)]
struct State {
    thread_mgr: Option<ITfThreadMgr>,
    tid: u32,
    focus_cookie: Option<u32>,
    composition: Option<ITfComposition>,
    session: Session,
    window: Option<CandidateWindow>,
    /// GUID atoms for [input, converted, focused] display attributes.
    atoms: [i32; 3],
    /// Context of the last key event (target for mouse selections).
    ctx: Option<ITfContext>,
    /// Taskbar input mode indicator.
    mode_button: Option<ComObject<ModeButton>>,
    /// "あ" / "A" badge near the caret.
    indicator: Option<ModeIndicator>,
    keymap: Keymap,
    /// Settings file times when last loaded.
    settings_stamp: [Option<std::time::SystemTime>; 2],
    /// Open/close compartment event sink: (compartment source, cookie).
    openclose_sink: Option<(ITfSource, u32)>,
}

#[implement(
    ITfTextInputProcessorEx,
    ITfTextInputProcessor,
    ITfKeyEventSink,
    ITfCompositionSink,
    ITfThreadFocusSink,
    ITfDisplayAttributeProvider,
    ITfCompartmentEventSink,
    ITfFnConfigure,
    ITfFunction
)]
pub struct TextService {
    st: RefCell<State>,
}

impl TextService {
    pub fn new() -> Self {
        crate::lock_module(true);
        Self { st: RefCell::new(State::default()) }
    }
}

impl Drop for TextService {
    fn drop(&mut self) {
        crate::lock_module(false);
    }
}

fn utf16_len(s: &str) -> i32 {
    s.encode_utf16().count() as i32
}

/// UTF-16 offset of byte offset `b` in `s`.
fn u16_at(s: &str, b: usize) -> i32 {
    utf16_len(&s[..b])
}

fn sub_range(range: &ITfRange, ec: u32, start: i32, end: i32) -> Result<ITfRange> {
    unsafe {
        let r = range.Clone()?;
        r.Collapse(ec, TF_ANCHOR_START)?;
        let mut moved = 0;
        r.ShiftEnd(ec, end, &mut moved, std::ptr::null())?;
        r.ShiftStart(ec, start, &mut moved, std::ptr::null())?;
        Ok(r)
    }
}

/// Up to 40 characters of document text immediately before `range`.
fn read_left_context(ec: u32, range: &ITfRange) -> Option<String> {
    unsafe {
        let r = range.Clone().ok()?;
        r.Collapse(ec, TF_ANCHOR_START).ok()?;
        let mut moved = 0;
        r.ShiftStart(ec, -(sweet_zenz::MAX_CONTEXT_CHARS as i32), &mut moved, std::ptr::null()).ok()?;
        let mut buf = [0u16; 64];
        let mut n = 0u32;
        r.GetText(ec, 0, &mut buf, &mut n).ok()?;
        Some(String::from_utf16_lossy(&buf[..n as usize]))
    }
}

fn set_selection(ctx: &ITfContext, ec: u32, range: &ITfRange) -> Result<()> {
    let sel = TF_SELECTION {
        range: std::mem::ManuallyDrop::new(Some(range.clone())),
        style: TF_SELECTIONSTYLE { ase: TF_AE_NONE, fInterimChar: BOOL(0) },
    };
    let r = unsafe { ctx.SetSelection(ec, &[sel.clone()]) };
    drop(std::mem::ManuallyDrop::into_inner(sel.range));
    r
}

impl TextService_Impl {
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
            st.keymap = sweet_engine::paths::load_keymap();
        }
    }

    /// Shows "あ" / "A" at the caret of the focused document.
    fn show_mode_indicator(&self, open: bool) {
        let (enabled, tm, tid) = {
            let Ok(st) = self.st.try_borrow() else { return };
            (st.session.options.mode_indicator, st.thread_mgr.clone(), st.tid)
        };
        let Some(tm) = tm.filter(|_| enabled) else { return };
        let ctx = unsafe { tm.GetFocus().and_then(|d| d.GetTop()).ok() };
        let this: ComObject<TextService> = self.to_object();
        let Some(ctx) = ctx else {
            if let Some((rc, owner)) = gui_caret() {
                this.indicator_at(open, rc, owner);
            }
            return;
        };
        let ctx2 = ctx.clone();
        let _ = edit::request_any(&ctx, tid, move |ec| {
            let found = unsafe { document_caret(ec, &ctx2) }.or_else(gui_caret);
            if let Some((rc, owner)) = found {
                this.indicator_at(open, rc, owner);
            }
            Ok(())
        });
    }

    fn indicator_at(&self, open: bool, caret: RECT, owner: Option<HWND>) {
        let Ok(mut st) = self.st.try_borrow_mut() else { return };
        if st.indicator.is_none() {
            st.indicator = ModeIndicator::new().ok();
        }
        if let Some(i) = &st.indicator {
            i.show(open, caret, owner);
        }
    }

    /// The taskbar mode indicator was clicked.
    fn on_mode_click(&self) {
        let _ = crate::guard(|| {
            let open = !self.is_open();
            if !open {
                let (active, ctx, tid) = {
                    let st = self.st.borrow();
                    (st.session.is_active(), st.ctx.clone(), st.tid)
                };
                if let (true, Some(ctx)) = (active, ctx) {
                    self.st.borrow_mut().session.commit_all();
                    let this: ComObject<TextService> = self.to_object();
                    let ctx2 = ctx.clone();
                    edit::request_any(&ctx, tid, move |ec| update_document(&this, ec, &ctx2))?;
                }
            }
            self.set_open(open);
            Ok(())
        });
    }

    fn translate(&self, vk: u32, lparam: isize) -> Option<Input> {
        let st = self.st.borrow();
        keys::translate(&st.keymap, vk, lparam, st.session.is_active())
    }

    /// Whether the key-down event would be consumed.
    fn wants_key(&self, vk: u32, lparam: isize) -> bool {
        let key = match self.translate(vk, lparam) {
            None => return false,
            Some(Input::Toggle(_)) => return true,
            Some(Input::Key(k)) => k,
        };
        if crate::converter().is_none() || !self.is_open() {
            return false;
        }
        if self.st.borrow().session.is_active() {
            return true;
        }
        matches!(key, sweet_engine::Key::Char(_) | sweet_engine::Key::Space)
    }

    /// Starts refining the live conversion in the background (reranker only).
    fn request_live_refinement(&self) {
        let Some(conv) = crate::converter() else { return };
        if !conv.has_reranker() {
            return;
        }
        let st = self.st.borrow();
        let (Some((reading, left_context)), Some(w)) = (st.session.live_request(), &st.window) else { return };
        let hwnd = w.raw();
        let r2 = reading.clone();
        crate::live::submit(crate::live::Job {
            reading,
            left_context,
            done: Box::new(move |surface| crate::candidate_window::post_live(hwnd, r2, surface)),
        });
    }

    /// A background live conversion result arrived (UI thread).
    fn on_live_result(&self, reading: String, surface: String) {
        let _ = crate::guard(|| {
            let Ok(mut st) = self.st.try_borrow_mut() else { return Ok(()) };
            if !st.session.apply_live(&reading, &surface) {
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
    fn update(&self, ctx: &ITfContext) -> Result<()> {
        let this: ComObject<TextService> = self.to_object();
        let tid = self.st.borrow().tid;
        let ctx2 = ctx.clone();
        edit::request(ctx, tid, move |ec| update_document(&this, ec, &ctx2))
    }

    fn handle_toggle(&self, ctx: &ITfContext, t: Toggle) -> Result<()> {
        let open = self.is_open();
        let new = match t {
            Toggle::Toggle => !open,
            Toggle::On => true,
            Toggle::Off => false,
        };
        if !new && self.st.borrow().session.is_active() {
            self.st.borrow_mut().session.commit_all();
            self.update(ctx)?;
        }
        if new != open {
            self.set_open(new);
        }
        Ok(())
    }
}

fn ensure_composition(
    this: &ComObject<TextService>,
    st: &mut State,
    ec: u32,
    ctx: &ITfContext,
) -> Result<ITfComposition> {
    if let Some(c) = &st.composition {
        return Ok(c.clone());
    }
    unsafe {
        let ins: ITfInsertAtSelection = ctx.cast()?;
        let range = ins.InsertTextAtSelection(ec, TF_IAS_QUERYONLY, &[])?;
        let cc: ITfContextComposition = ctx.cast()?;
        let sink: ITfCompositionSink = this.to_interface();
        let comp = cc.StartComposition(ec, &range, &sink)?;
        st.composition = Some(comp.clone());
        Ok(comp)
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
    w.set_on_live(move |r, s| t.on_live_result(r, s));
    Some(w)
}

/// Whether the thread hosts an immersive (UWP / shell) application.
fn immersive(tm: &ITfThreadMgr) -> bool {
    unsafe { tm.cast::<ITfThreadMgrEx>().and_then(|t| t.GetActiveFlags()).is_ok_and(|f| f & TF_TMF_IMMERSIVEMODE != 0) }
}

fn update_document(this: &ComObject<TextService>, ec: u32, ctx: &ITfContext) -> Result<()> {
    let Ok(mut st) = this.st.try_borrow_mut() else { return Ok(()) };
    let st = &mut *st;
    let commit = st.session.take_commit();
    let pre = st.session.preedit();
    unsafe {
        let attr_prop = ctx.GetProperty(&GUID_PROP_ATTRIBUTE)?;
        if !commit.is_empty() {
            let comp = ensure_composition(this, st, ec, ctx)?;
            let range = comp.GetRange()?;
            let text: Vec<u16> = commit.encode_utf16().collect();
            range.SetText(ec, 0, &text)?;
            let _ = attr_prop.Clear(ec, &range);
            range.Collapse(ec, TF_ANCHOR_END)?;
            set_selection(ctx, ec, &range)?;
            comp.EndComposition(ec)?;
            st.composition = None;
        }
        if pre.text.is_empty() {
            if let Some(comp) = st.composition.take() {
                let range = comp.GetRange()?;
                range.SetText(ec, 0, &[])?;
                comp.EndComposition(ec)?;
            }
            if let Some(w) = &st.window {
                w.hide();
            }
            return Ok(());
        }
        let comp = ensure_composition(this, st, ec, ctx)?;
        let range = comp.GetRange()?;
        if let Some(c) = read_left_context(ec, &range) {
            st.session.set_left_context(&c);
        }
        let text: Vec<u16> = pre.text.encode_utf16().collect();
        range.SetText(ec, 0, &text)?;
        let _ = attr_prop.Clear(ec, &range);
        let mut focused: Option<ITfRange> = None;
        for &(s, e, attr) in &pre.spans {
            let r = sub_range(&range, ec, u16_at(&pre.text, s), u16_at(&pre.text, e))?;
            let atom = match attr {
                SpanAttr::Input => st.atoms[0],
                SpanAttr::Converted => st.atoms[1],
                SpanAttr::Focused => st.atoms[2],
            };
            let _ = attr_prop.SetValue(ec, &r, &VARIANT::from(atom));
            if attr == SpanAttr::Focused {
                focused = Some(r);
            }
        }
        let caret = u16_at(&pre.text, pre.caret);
        let caret_range = sub_range(&range, ec, caret, caret)?;
        set_selection(ctx, ec, &caret_range)?;

        match st.session.candidates() {
            Some(list) => {
                let target = focused.unwrap_or(range);
                let mut rc = RECT::default();
                let mut clipped = BOOL(0);
                if let Ok(view) = ctx.GetActiveView() {
                    let _ = view.GetTextExt(ec, &target, &mut rc, &mut clipped);
                }
                // Own the window by the application window holding the text.
                let owner = ctx
                    .GetActiveView()
                    .and_then(|v| v.GetWnd())
                    .ok()
                    .filter(|h| !h.is_invalid())
                    .or_else(|| Some(GetFocus()).filter(|h| !h.is_invalid()));
                if st.window.is_none() {
                    st.window = make_window(this, owner);
                }
                if let Some(w) = &st.window {
                    if let Some(o) = owner {
                        w.set_owner(o);
                    }
                    w.show(&list, rc);
                }
            }
            None => {
                if let Some(w) = &st.window {
                    w.hide();
                }
            }
        }
    }
    Ok(())
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
                        let _ = km.UnadviseKeyEventSink(st.tid);
                    }
                    if let (Some(cookie), Ok(src)) = (st.focus_cookie.take(), tm.cast::<ITfSource>()) {
                        let _ = src.UnadviseSink(cookie);
                    }
                }
            }
            if let Some((src, cookie)) = st.openclose_sink.take() {
                unsafe {
                    let _ = src.UnadviseSink(cookie);
                }
            }
            st.composition = None;
            st.session.reset();
            st.window = None;
            st.indicator = None;
            st.ctx = None;
            Ok(())
        })
    }
}

impl ITfTextInputProcessorEx_Impl for TextService_Impl {
    fn ActivateEx(&self, ptim: Ref<ITfThreadMgr>, tid: u32, _flags: u32) -> Result<()> {
        crate::guard(|| {
            let tm = ptim.ok()?.clone();
            {
                let mut st = self.st.borrow_mut();
                st.thread_mgr = Some(tm.clone());
                st.tid = tid;
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
                st.keymap = sweet_engine::paths::load_keymap();
                st.settings_stamp = crate::settings_stamp();
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
            self.set_open(true);
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
            // Switched to Sweet IME (e.g. Win+Space): show the mode.
            self.show_mode_indicator(self.is_open());
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

/// Screen rectangle of the selection (caret) in `ctx`, and the window
/// that shows it.
unsafe fn document_caret(ec: u32, ctx: &ITfContext) -> Option<(RECT, Option<HWND>)> {
    unsafe {
        let mut sel = [TF_SELECTION::default()];
        let mut fetched = 0u32;
        ctx.GetSelection(ec, TF_DEFAULT_SELECTION, &mut sel, &mut fetched).ok()?;
        if fetched == 0 {
            return None;
        }
        let range = std::mem::ManuallyDrop::into_inner(std::mem::take(&mut sel[0].range))?;
        let view = ctx.GetActiveView().ok()?;
        let mut rc = RECT::default();
        let mut clipped = BOOL(0);
        view.GetTextExt(ec, &range, &mut rc, &mut clipped).ok()?;
        if rc.left == 0 && rc.top == 0 && rc.right == 0 && rc.bottom == 0 {
            return None;
        }
        let owner = view.GetWnd().ok().filter(|h| !h.is_invalid());
        Some((rc, owner))
    }
}

/// The Win32 caret of this thread (apps without TSF text ranges).
fn gui_caret() -> Option<(RECT, Option<HWND>)> {
    use windows::Win32::Graphics::Gdi::ClientToScreen;
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::WindowsAndMessaging::{GUITHREADINFO, GetGUIThreadInfo};
    unsafe {
        let mut gi = GUITHREADINFO { cbSize: std::mem::size_of::<GUITHREADINFO>() as u32, ..Default::default() };
        GetGUIThreadInfo(GetCurrentThreadId(), &mut gi).ok()?;
        if gi.hwndCaret.is_invalid() {
            return None;
        }
        let mut tl = POINT { x: gi.rcCaret.left, y: gi.rcCaret.top };
        let mut br = POINT { x: gi.rcCaret.right, y: gi.rcCaret.bottom };
        let _ = ClientToScreen(gi.hwndCaret, &mut tl);
        let _ = ClientToScreen(gi.hwndCaret, &mut br);
        Some((RECT { left: tl.x, top: tl.y, right: br.x, bottom: br.y }, Some(gi.hwndCaret)))
    }
}

impl ITfKeyEventSink_Impl for TextService_Impl {
    fn OnSetFocus(&self, _foreground: BOOL) -> Result<()> {
        Ok(())
    }

    fn OnTestKeyDown(&self, _pic: Ref<ITfContext>, wparam: WPARAM, lparam: LPARAM) -> Result<BOOL> {
        crate::guard(|| Ok(BOOL(self.wants_key(wparam.0 as u32, lparam.0) as i32)))
    }

    fn OnTestKeyUp(&self, _pic: Ref<ITfContext>, _wparam: WPARAM, _lparam: LPARAM) -> Result<BOOL> {
        Ok(BOOL(0))
    }

    fn OnKeyDown(&self, pic: Ref<ITfContext>, wparam: WPARAM, lparam: LPARAM) -> Result<BOOL> {
        crate::guard(|| {
            let vk = wparam.0 as u32;
            let ctx = pic.ok()?.clone();
            self.st.borrow_mut().ctx = Some(ctx.clone());
            let key = match self.translate(vk, lparam.0) {
                None => return Ok(BOOL(0)),
                Some(Input::Toggle(t)) => {
                    self.handle_toggle(&ctx, t)?;
                    return Ok(BOOL(1));
                }
                Some(Input::Key(k)) => k,
            };
            if !self.wants_key(vk, lparam.0) {
                return Ok(BOOL(0));
            }
            let Some(conv) = crate::converter() else { return Ok(BOOL(0)) };
            let consumed = self.st.borrow_mut().session.handle(conv, key);
            if consumed {
                if let Some(i) = &self.st.borrow().indicator {
                    i.hide();
                }
                self.update(&ctx)?;
                self.request_live_refinement();
            }
            Ok(BOOL(consumed as i32))
        })
    }

    fn OnKeyUp(&self, _pic: Ref<ITfContext>, _wparam: WPARAM, _lparam: LPARAM) -> Result<BOOL> {
        Ok(BOOL(0))
    }

    fn OnPreservedKey(&self, _pic: Ref<ITfContext>, _rguid: *const GUID) -> Result<BOOL> {
        Ok(BOOL(0))
    }
}

impl ITfCompositionSink_Impl for TextService_Impl {
    fn OnCompositionTerminated(&self, _ec: u32, _comp: Ref<ITfComposition>) -> Result<()> {
        // The application ended the composition (e.g. clicked elsewhere).
        // The text stays in the document as-is.
        if let Ok(mut st) = self.st.try_borrow_mut() {
            st.composition = None;
            st.session.reset();
            if let Some(w) = &st.window {
                w.hide();
            }
        }
        Ok(())
    }
}

impl ITfThreadFocusSink_Impl for TextService_Impl {
    fn OnSetThreadFocus(&self) -> Result<()> {
        self.reload_settings_if_changed();
        Ok(())
    }

    fn OnKillThreadFocus(&self) -> Result<()> {
        if let Ok(st) = self.st.try_borrow() {
            if let Some(w) = &st.window {
                w.hide();
            }
            if let Some(i) = &st.indicator {
                i.hide();
            }
        }
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
