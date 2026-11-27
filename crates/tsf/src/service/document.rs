//! TSF composition ranges, document edits, and caret geometry.

use sweet_engine::SpanAttr;
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Input::KeyboardAndMouse::GetFocus;
use windows::Win32::UI::TextServices::{
    GUID_PROP_ATTRIBUTE, GUID_PROP_INPUTSCOPE, IS_PASSWORD, IS_PRIVATE, ITfComposition,
    ITfCompositionSink, ITfContext, ITfContextComposition, ITfContextView, ITfInputScope,
    ITfInsertAtSelection, ITfRange, InputScope, TF_AE_NONE, TF_ANCHOR_END, TF_ANCHOR_START,
    TF_DEFAULT_SELECTION, TF_IAS_QUERYONLY, TF_SELECTION, TF_SELECTIONSTYLE,
};
use windows_core::{BOOL, ComObject, IUnknown, Interface, Result};

use super::{State, TextService, make_window};

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
        r.ShiftStart(
            ec,
            -(sweet_zenz::MAX_CONTEXT_CHARS as i32),
            &mut moved,
            std::ptr::null(),
        )
        .ok()?;
        let mut buf = [0u16; 64];
        let mut n = 0u32;
        r.GetText(ec, 0, &mut buf, &mut n).ok()?;
        if n == 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&buf[..n as usize]))
    }
}

fn set_selection(ctx: &ITfContext, ec: u32, range: &ITfRange) -> Result<()> {
    let sel = TF_SELECTION {
        range: std::mem::ManuallyDrop::new(Some(range.clone())),
        style: TF_SELECTIONSTYLE {
            ase: TF_AE_NONE,
            fInterimChar: BOOL(0),
        },
    };
    let r = unsafe { ctx.SetSelection(ec, &[sel.clone()]) };
    drop(std::mem::ManuallyDrop::into_inner(sel.range));
    r
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

pub(super) fn update_document(
    this: &ComObject<TextService>,
    ec: u32,
    ctx: &ITfContext,
) -> Result<()> {
    let Ok(mut st) = this.st.try_borrow_mut() else {
        return Ok(());
    };
    let st = &mut *st;
    let r = update_document_inner(this, st, ec, ctx);
    crate::set_busy(st.composition.is_some());
    r
}

/// Removes `text` just before the caret, if it is there (the commit being
/// taken back). Returns whether it was.
fn delete_before_caret(ctx: &ITfContext, ec: u32, text: &str) -> Result<bool> {
    let want: Vec<u16> = text.encode_utf16().collect();
    unsafe {
        let ins: ITfInsertAtSelection = ctx.cast()?;
        let range = ins.InsertTextAtSelection(ec, TF_IAS_QUERYONLY, &[])?;
        range.Collapse(ec, TF_ANCHOR_START)?;
        let mut moved = 0;
        range.ShiftStart(ec, -(want.len() as i32), &mut moved, std::ptr::null())?;
        let mut buf = vec![0u16; want.len() + 1];
        let mut n = 0u32;
        range.GetText(ec, 0, &mut buf, &mut n)?;
        if buf[..n as usize] != want[..] {
            return Ok(false);
        }
        range.SetText(ec, 0, &[])?;
        range.Collapse(ec, TF_ANCHOR_END)?;
        set_selection(ctx, ec, &range)?;
        Ok(true)
    }
}

/// Whether the field at the selection is a password or a private browser
/// window (input scopes IS_PASSWORD, IS_PRIVATE).
pub(super) fn private_field(ctx: &ITfContext, ec: u32) -> Result<bool> {
    unsafe {
        let ins: ITfInsertAtSelection = ctx.cast()?;
        let range = ins.InsertTextAtSelection(ec, TF_IAS_QUERYONLY, &[])?;
        let value = ctx
            .GetAppProperty(&GUID_PROP_INPUTSCOPE)?
            .GetValue(ec, &range)?;
        // Ordinary editors often have no input scope set on the range.
        if value.is_empty() {
            return Ok(false);
        }
        let scope: ITfInputScope = IUnknown::try_from(&value)?.cast()?;
        let mut list: *mut InputScope = std::ptr::null_mut();
        let mut n = 0u32;
        scope.GetInputScopes(&mut list, &mut n)?;
        if list.is_null() {
            return Ok(false);
        }
        let private = std::slice::from_raw_parts(list, n as usize)
            .iter()
            .any(|s| *s == IS_PASSWORD || *s == IS_PRIVATE);
        windows::Win32::System::Com::CoTaskMemFree(Some(list as *const std::ffi::c_void));
        Ok(private)
    }
}

fn update_document_inner(
    this: &ComObject<TextService>,
    st: &mut State,
    ec: u32,
    ctx: &ITfContext,
) -> Result<()> {
    // A new composition: is this field private?
    if st.composition.is_none() {
        let opts = st.session.options;
        let private = (opts.private_fields || opts.show_typed_input)
            .then(|| private_field(ctx, ec).ok())
            .flatten();
        st.session.private = opts.private_fields && private == Some(true);
        // A settings reload can enable the preview during an existing
        // composition. Only allow it if we checked at composition start.
        st.typed_input_allowed = opts.show_typed_input && private == Some(false);
    }
    if let Some(undo) = st.session.take_undo() {
        // The caret moved or the text changed since: keep the document.
        if st.composition.is_some() || !delete_before_caret(ctx, ec, &undo).unwrap_or(false) {
            st.session.reset();
        }
    }
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
        for &(s, e, attr) in &pre.spans {
            let r = sub_range(&range, ec, u16_at(&pre.text, s), u16_at(&pre.text, e))?;
            let atom = match attr {
                SpanAttr::Input => st.atoms[0],
                SpanAttr::Converted => st.atoms[1],
                SpanAttr::Focused => st.atoms[2],
            };
            let _ = attr_prop.SetValue(ec, &r, &VARIANT::from(atom));
        }
        let caret = u16_at(&pre.text, pre.caret);
        let caret_range = sub_range(&range, ec, caret, caret)?;
        set_selection(ctx, ec, &caret_range)?;

        refresh_candidates(this, st, ec, ctx, false)?;
    }
    Ok(())
}

/// Looks taken for an unplaced composition, about a second in all.
const RELAYOUT_TRIES: u32 = 8;

pub(super) fn refresh_candidates(
    this: &ComObject<TextService>,
    st: &mut State,
    ec: u32,
    ctx: &ITfContext,
    retry: bool,
) -> Result<()> {
    if !retry {
        st.relayout_tries = 0;
    }
    let list = st.session.candidates();
    let input = (st.session.options.show_typed_input && st.typed_input_allowed)
        .then(|| st.session.typed_input());
    let input = input.as_deref().filter(|text| !text.is_empty());
    let Some(comp) = st.composition.as_ref().filter(|_| list.is_some() || input.is_some()) else {
        if let Some(w) = &st.window {
            // While typing, predictions may come back on the next key.
            if st.session.is_converting() {
                w.hide()
            } else {
                w.hide_soon()
            }
        }
        return Ok(());
    };
    let show = |w: &crate::candidate_window::CandidateWindow, rc| {
        w.show(
            list.as_ref(),
            input,
            rc,
            st.session.options.gaming_mode,
            st.session.options.window_fade,
            &st.candidate_font,
        );
    };
    let range = unsafe { comp.GetRange()? };
    let pre = st.session.preedit();
    let target = pre
        .spans
        .iter()
        .find(|span| span.2 == SpanAttr::Focused)
        .and_then(|&(start, end, _)| {
            sub_range(&range, ec, u16_at(&pre.text, start), u16_at(&pre.text, end)).ok()
        });
    let caret = u16_at(&pre.text, pre.caret);
    let anchor = unsafe {
        ctx.GetActiveView()
            .ok()
            .and_then(|view| {
                let owner = view.GetWnd().ok().filter(|h| !h.is_invalid());
                let bounds = range_bounds(&view, ec, target.as_ref().unwrap_or(&range), owner)
                    .or_else(|| {
                        let selection = sub_range(&range, ec, caret, caret).ok()?;
                        range_bounds(&view, ec, &selection, owner)
                    });
                bounds.map(|rc| (rc, owner))
            })
            .or_else(gui_caret)
    };
    let Some((rc, owner)) = anchor else {
        // The host has not laid out the composition yet, and may not say
        // when it has: keep a showing list where it is and look again
        // shortly, then fall back to where the list was last shown.
        if st.window.is_none() {
            st.window = make_window(
                this,
                Some(unsafe { GetFocus() }).filter(|h| !h.is_invalid()),
                st.bitmap_candidates,
            );
        }
        if let Some(w) = &st.window {
            st.relayout_tries += 1;
            if let Some(rc) = w.visible_anchor() {
                show(w, rc);
            } else if st.relayout_tries > RELAYOUT_TRIES {
                if let Some(rc) = w.last_anchor() {
                    show(w, rc);
                }
            }
            if st.relayout_tries <= RELAYOUT_TRIES {
                crate::candidate_window::relayout_after(w.raw(), 25 * st.relayout_tries);
            }
        }
        return Ok(());
    };
    st.relayout_tries = 0;
    let owner = owner.or_else(|| Some(unsafe { GetFocus() }).filter(|h| !h.is_invalid()));
    if st.window.is_none() {
        st.window = make_window(this, owner, st.bitmap_candidates);
    }
    if let Some(w) = &st.window {
        if let Some(owner) = owner {
            w.set_owner(owner);
        }
        show(w, rc);
    }
    Ok(())
}

/// Screen rectangle of the selection (caret) in `ctx`, and the window
/// that shows it.
pub(super) unsafe fn document_caret(ec: u32, ctx: &ITfContext) -> Option<(RECT, Option<HWND>)> {
    unsafe {
        let mut sel = [TF_SELECTION::default()];
        let mut fetched = 0u32;
        ctx.GetSelection(ec, TF_DEFAULT_SELECTION, &mut sel, &mut fetched)
            .ok()?;
        if fetched == 0 {
            return None;
        }
        let range = std::mem::ManuallyDrop::into_inner(std::mem::take(&mut sel[0].range))?;
        let view = ctx.GetActiveView().ok()?;
        let owner = view.GetWnd().ok().filter(|h| !h.is_invalid());
        Some((range_bounds(&view, ec, &range, owner)?, owner))
    }
}

fn range_bounds(
    view: &ITfContextView,
    ec: u32,
    range: &ITfRange,
    owner: Option<HWND>,
) -> Option<RECT> {
    unsafe {
        let mut rc = RECT::default();
        let mut clipped = BOOL(0);
        view.GetTextExt(ec, range, &mut rc, &mut clipped).ok()?;
        // A clipped range (partly scrolled out of view) still places the list.
        if rc.bottom <= rc.top || rc.right < rc.left {
            return None;
        }
        // Ignore bogus coordinates from hosts that have not laid out the text.
        if let Some(w) = owner {
            let mut wr = RECT::default();
            if windows::Win32::UI::WindowsAndMessaging::GetWindowRect(w, &mut wr).is_ok()
                && (rc.left < wr.left - 8
                    || rc.left > wr.right + 8
                    || rc.bottom < wr.top - 8
                    || rc.top > wr.bottom + 8)
            {
                return None;
            }
        }
        Some(rc)
    }
}

/// The Win32 caret of this thread (apps without TSF text ranges).
pub(super) fn gui_caret() -> Option<(RECT, Option<HWND>)> {
    use windows::Win32::Graphics::Gdi::ClientToScreen;
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::WindowsAndMessaging::{GUITHREADINFO, GetGUIThreadInfo};
    unsafe {
        let mut gi = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        GetGUIThreadInfo(GetCurrentThreadId(), &mut gi).ok()?;
        if gi.hwndCaret.is_invalid() {
            return None;
        }
        let mut tl = POINT {
            x: gi.rcCaret.left,
            y: gi.rcCaret.top,
        };
        let mut br = POINT {
            x: gi.rcCaret.right,
            y: gi.rcCaret.bottom,
        };
        let _ = ClientToScreen(gi.hwndCaret, &mut tl);
        let _ = ClientToScreen(gi.hwndCaret, &mut br);
        Some((
            RECT {
                left: tl.x,
                top: tl.y,
                right: br.x,
                bottom: br.y,
            },
            Some(gi.hwndCaret),
        ))
    }
}
