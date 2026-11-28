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

/// Handles a key event. `sym` is an X keysym, `unicode` the character it
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
pub unsafe extern "C" fn sweet_reset_modifier_tap(p: *mut SweetContext) {
    unsafe { ctx(p) }.modifier_tap = Default::default();
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

#[repr(C)]
pub struct SweetUiTheme {
    pub bg: [u8; 3],
    pub text: [u8; 3],
    pub sub: [u8; 3],
    pub hover: [u8; 3],
    pub accent: [u8; 3],
    pub border: [u8; 3],
    pub bg_alpha: f32,
    pub hover_alpha: f32,
    pub panel_padding: u8,
    pub dark: bool,
    pub gaming_mode: bool,
    pub window_fade: bool,
}

/// Loads the active Sweet IME theme (including custom User Theme and opacity)
/// for the native fcitx5 UI addon.
///
/// # Safety
/// `out` must be a valid writable pointer to [`SweetUiTheme`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sweet_ui_load_theme(out: *mut SweetUiTheme) {
    if out.is_null() {
        return;
    }
    let theme = guard(
        SweetUiTheme {
            bg: [0x2c, 0x2c, 0x2c],
            text: [0xff, 0xff, 0xff],
            sub: [0xc5, 0xc5, 0xc5],
            hover: [0xff, 0xff, 0xff],
            accent: [0x60, 0xcd, 0xff],
            border: [0x40, 0x40, 0x40],
            bg_alpha: 1.0,
            hover_alpha: 0.08,
            panel_padding: 6,
            dark: true,
            gaming_mode: false,
            window_fade: true,
        },
        || {
            let opts = sweet_engine::paths::load_options();
            let c = if opts.theme == sweet_engine::Theme::Custom {
                sweet_engine::paths::load_user_theme().colors()
            } else {
                opts.theme.colors(false)
            };
            let accent = c.accent.unwrap_or(if c.dark {
                sweet_engine::theme::Rgb(0x60, 0xcd, 0xff)
            } else {
                sweet_engine::theme::Rgb(0x00, 0x5f, 0xb8)
            });
            SweetUiTheme {
                bg: [c.bg.0, c.bg.1, c.bg.2],
                text: [c.text.0, c.text.1, c.text.2],
                sub: [c.sub.0, c.sub.1, c.sub.2],
                hover: [c.hover.0, c.hover.1, c.hover.2],
                accent: [accent.0, accent.1, accent.2],
                border: [c.border.0, c.border.1, c.border.2],
                bg_alpha: c.bg_alpha.clamp(sweet_engine::theme::MIN_BG_ALPHA, 1.0),
                hover_alpha: c.hover_alpha.clamp(0.0, 1.0),
                panel_padding: c.panel_padding.clamp(
                    sweet_engine::theme::MIN_PANEL_PADDING,
                    sweet_engine::theme::MAX_PANEL_PADDING,
                ),
                dark: c.dark,
                gaming_mode: opts.gaming_mode,
                window_fade: opts.window_fade,
            }
        },
    );
    unsafe {
        *out = theme;
    }
}

/// Tells whether the focused field may contribute Alpha feedback. Call before
/// each key press; it only takes effect when a composition starts.
///
/// # Safety
/// `p` is a live context.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sweet_set_feedback_field(p: *mut SweetContext, eligible: bool) {
    let c = unsafe { ctx(p) };
    guard((), || c.set_feedback_field(eligible))
}

/// Sends pending Alpha feedback without waiting for the batch window.
#[unsafe(no_mangle)]
pub extern "C" fn sweet_feedback_flush() {
    guard((), sweet_engine::alpha_feedback::flush_soon)
}
