//! C entry points and the ownership contract for pointers passed to fcitx5.

use std::ffi::{CStr, c_char, c_void};

use super::{Notify, NotifyFn, SweetContext, converter};

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
    allow_cloud: bool,
) -> u32 {
    let c = unsafe { ctx(p) };
    guard(0, || c.process_key(sym, unicode, mods, allow_cloud))
}

/// # Safety
/// `p` is a live context.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sweet_apply_live(p: *mut SweetContext, allow_cloud: bool) -> bool {
    let c = unsafe { ctx(p) };
    guard(false, || c.apply_live(allow_cloud))
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
