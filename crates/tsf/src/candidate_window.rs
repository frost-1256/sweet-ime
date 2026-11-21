//! Fluent-style candidate window drawn with Direct2D / DirectWrite.
//!
//! The window never takes activation; it is a top-most tool popup with
//! DWM rounded corners, follows the configured theme (by default the system light/dark
//! theme and accent color), fades in and animates the selection highlight.

use std::cell::RefCell;
use std::sync::Once;
use std::time::Instant;

use sweet_engine::theme::Rgb;
use sweet_engine::{CandidateList, ListKind};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D_SIZE_U, D2D1_ALPHA_MODE_UNKNOWN, D2D1_COLOR_F, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT, D2D1_FACTORY_TYPE_SINGLE_THREADED,
    D2D1_HWND_RENDER_TARGET_PROPERTIES, D2D1_PRESENT_OPTIONS_IMMEDIATELY, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_ROUNDED_RECT, D2D1CreateFactory, ID2D1Factory, ID2D1HwndRenderTarget,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_METRICS,
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, IDWriteTextLayout,
};
use windows::Win32::Graphics::Dwm::{
    DWMWA_BORDER_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, InvalidateRect, GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO,
    MonitorFromPoint, PAINTSTRUCT, ValidateRect,
};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CS_DROPSHADOW, CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow,
    GWLP_USERDATA, GetWindowLongPtrW, HWND_TOPMOST, IDC_ARROW, KillTimer, LWA_ALPHA, LoadCursorW,
    MA_NOACTIVATE, RegisterClassExW, SW_HIDE, SWP_NOACTIVATE, SWP_SHOWWINDOW, SetLayeredWindowAttributes,
    PostMessageW, SetTimer, SetWindowLongPtrW, GWLP_HWNDPARENT, WM_APP, SetWindowPos, ShowWindow, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_PAINT, WM_TIMER,
    WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows_core::{HSTRING, PCWSTR, Result, w};

const CLASS_NAME: PCWSTR = w!("SweetImeCandidateWindow");
const TIMER_ID: usize = 1;

// Layout in DIPs.
const PAD: f32 = 4.0;
const ROW_H: f32 = 32.0;
const INDEX_W: f32 = 28.0;
const TEXT_PAD_R: f32 = 16.0;
const FOOTER_H: f32 = 24.0;
const FONT_SIZE: f32 = 16.0;
const MIN_W: f32 = 140.0;
const FADE_MS: f32 = 90.0;
const SLIDE_MS: f32 = 70.0;

#[derive(Clone, Copy)]
struct Palette {
    bg: D2D1_COLOR_F,
    text: D2D1_COLOR_F,
    sub: D2D1_COLOR_F,
    hover: D2D1_COLOR_F,
    accent: D2D1_COLOR_F,
    border: COLORREF,
}

fn rgb(r: u8, g: u8, b: u8, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a }
}

pub(crate) fn read_dword(path: PCWSTR, name: PCWSTR) -> Option<u32> {
    let mut v = 0u32;
    let mut size = 4u32;
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            path,
            name,
            RRF_RT_REG_DWORD,
            None,
            Some(&mut v as *mut u32 as *mut _),
            Some(&mut size),
        )
    };
    r.is_ok().then_some(v)
}

/// The Windows app theme (`AppsUseLightTheme`); light when unset.
pub(crate) fn system_light() -> bool {
    read_dword(
        w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
        w!("AppsUseLightTheme"),
    )
    .unwrap_or(1)
        != 0
}

fn palette() -> Palette {
    let c = crate::theme_colors();
    let col = |v: Rgb, a: f32| rgb(v.0, v.1, v.2, a);
    let accent = match c.accent {
        Some(v) => col(v, 1.0),
        // AccentColor is stored as 0xAABBGGRR.
        None => match read_dword(w!("Software\\Microsoft\\Windows\\DWM"), w!("AccentColor")) {
            Some(v) => {
                let (r, g, b) = ((v & 0xff) as u8, ((v >> 8) & 0xff) as u8, ((v >> 16) & 0xff) as u8);
                if c.dark {
                    // Lighten for dark backgrounds.
                    let l = |c: u8| c + ((255 - c) as f32 * 0.45) as u8;
                    rgb(l(r), l(g), l(b), 1.0)
                } else {
                    rgb(r, g, b, 1.0)
                }
            }
            None if c.dark => rgb(0x60, 0xcd, 0xff, 1.0),
            None => rgb(0x00, 0x5f, 0xb8, 1.0),
        },
    };
    let b = c.border;
    Palette {
        bg: col(c.bg, 1.0),
        text: col(c.text, 1.0),
        sub: col(c.sub, 1.0),
        hover: col(c.hover, c.hover_alpha),
        accent,
        // COLORREF is 0x00BBGGRR.
        border: COLORREF(u32::from(b.0) | u32::from(b.1) << 8 | u32::from(b.2) << 16),
    }
}

struct State {
    d2d: ID2D1Factory,
    dwrite: IDWriteFactory,
    target: Option<ID2D1HwndRenderTarget>,
    format: IDWriteTextFormat,
    small: IDWriteTextFormat,
    pal: Palette,
    items: Vec<String>,
    rows: Vec<Option<(IDWriteTextLayout, f32)>>,
    selected: usize,
    page_start: usize,
    page_len: usize,
    total: usize,
    /// Showing predictions while typing (no selection; Tab enters the list).
    suggest: bool,
    scale: f32,
    width: f32,
    /// Animated highlight row (fractional) and animation start.
    anim_from: f32,
    anim_start: Instant,
    shown_at: Instant,
    visible: bool,
    /// Called with the absolute candidate index when a row is clicked.
    on_click: Option<std::rc::Rc<dyn Fn(usize)>>,
    /// Called with (reading, surface) when a background live conversion
    /// result arrives.
    on_live: Option<std::rc::Rc<dyn Fn(String, String)>>,
}

/// Results from the live-conversion worker, keyed by window handle.
static LIVE_MAILBOX: std::sync::Mutex<Vec<(isize, String, String)>> = std::sync::Mutex::new(Vec::new());

/// Delivers a live conversion result to the window `hwnd` (any thread).
pub fn post_live(hwnd: isize, reading: String, surface: String) {
    let mut m = LIVE_MAILBOX.lock().unwrap_or_else(|e| e.into_inner());
    m.retain(|(h, _, _)| *h != hwnd);
    m.push((hwnd, reading, surface));
    drop(m);
    unsafe {
        let _ = PostMessageW(Some(HWND(hwnd as *mut _)), WM_APP_LIVE, WPARAM(0), LPARAM(0));
    }
}

impl State {
    fn highlight_row(&self) -> f32 {
        let target = (self.selected - self.page_start) as f32;
        let t = (self.anim_start.elapsed().as_secs_f32() * 1000.0 / SLIDE_MS).min(1.0);
        let e = 1.0 - (1.0 - t).powi(3); // ease-out cubic
        self.anim_from + (target - self.anim_from) * e
    }

    fn animating(&self) -> bool {
        self.anim_start.elapsed().as_secs_f32() * 1000.0 < SLIDE_MS
            || self.shown_at.elapsed().as_secs_f32() * 1000.0 < FADE_MS
    }
}

thread_local! {
    // The window lives on the host's UI thread; state is per-thread.
    static STATES: RefCell<Vec<(isize, Box<State>)>> = const { RefCell::new(Vec::new()) };
}

fn with_state<R>(hwnd: HWND, f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATES.with(|s| {
        let mut s = s.borrow_mut();
        s.iter_mut().find(|(h, _)| *h == hwnd.0 as isize).map(|(_, st)| f(st))
    })
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

pub struct CandidateWindow {
    hwnd: HWND,
    owner: std::cell::Cell<isize>,
}

fn register_class() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_DROPSHADOW | CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: crate::dll_instance(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        RegisterClassExW(&wc);
    });
}

impl CandidateWindow {
    /// Creates the window owned by `owner` (the application window the
    /// text is in). Owned windows stay above their owner, including in
    /// immersive hosts such as Windows Search, where an unowned topmost
    /// window would end up behind the host's UI.
    pub fn new(owner: Option<HWND>) -> Result<Self> {
        register_class();
        unsafe {
            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED,
                CLASS_NAME,
                PCWSTR::null(),
                WS_POPUP,
                0,
                0,
                1,
                1,
                owner,
                None,
                Some(crate::dll_instance()),
                None,
            )?;
            let pref = DWMWCP_ROUND;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &pref as *const _ as *const _,
                4,
            );
            let d2d: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let font = HSTRING::from("Yu Gothic UI");
            let locale = HSTRING::from("ja-jp");
            let format = dwrite.CreateTextFormat(
                &font, None, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL, FONT_SIZE, &locale,
            )?;
            let small = dwrite.CreateTextFormat(
                &HSTRING::from("Segoe UI Variable Text"), None, DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, 12.0, &locale,
            )?;
            let state = State {
                d2d,
                dwrite,
                target: None,
                format,
                small,
                pal: palette(),
                items: Vec::new(),
                rows: Vec::new(),
                selected: 0,
                page_start: 0,
                page_len: 0,
                total: 0,
                suggest: false,
                scale: 1.0,
                width: MIN_W,
                anim_from: 0.0,
                anim_start: Instant::now(),
                shown_at: Instant::now(),
                visible: false,
                on_click: None,
                on_live: None,
            };
            STATES.with(|s| s.borrow_mut().push((hwnd.0 as isize, Box::new(state))));
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 1);
            Ok(Self { hwnd, owner: std::cell::Cell::new(owner.map_or(0, |h| h.0 as isize)) })
        }
    }

    /// Changes the owner window (e.g. focus moved to another window of the
    /// same thread).
    pub fn set_owner(&self, owner: HWND) {
        if self.owner.get() != owner.0 as isize {
            self.owner.set(owner.0 as isize);
            unsafe {
                SetWindowLongPtrW(self.hwnd, GWLP_HWNDPARENT, owner.0 as _);
            }
        }
    }

    /// Shows `list` next to `anchor` (screen coordinates of the focused text).
    pub fn show(&self, list: &CandidateList, anchor: RECT) {
        let hwnd = self.hwnd;
        let geom = with_state(hwnd, |st| unsafe {
            let dpi = GetDpiForWindow(hwnd);
            st.scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
            let page_start = list.page_start();
            let page_end = (page_start + list.page_size).min(list.items.len());
            let was_visible = st.visible;
            let prev_row = if was_visible && st.page_start == page_start {
                st.highlight_row()
            } else {
                (list.selected - page_start) as f32
            };
            let items = &list.items[page_start..page_end];
            if st.items != items || st.rows.iter().any(Option::is_none) {
                st.items = items.to_vec();
                let mut max_width: f32 = 0.0;
                st.rows = items
                    .iter()
                    .map(|item| {
                        let layout = st
                            .dwrite
                            .CreateTextLayout(&wide(item), &st.format, 2000.0, ROW_H)
                            .ok()?;
                        let mut metrics = DWRITE_TEXT_METRICS::default();
                        let _ = layout.GetMetrics(&mut metrics);
                        max_width = max_width.max(metrics.widthIncludingTrailingWhitespace);
                        Some((layout, metrics.height))
                    })
                    .collect();
                st.width = (PAD * 2.0 + INDEX_W + max_width + TEXT_PAD_R)
                    .max(MIN_W)
                    .ceil();
            }
            st.page_start = page_start;
            st.page_len = page_end - page_start;
            st.selected = list.selected;
            st.total = list.items.len();
            st.suggest = list.kind == ListKind::Suggestion;
            st.anim_from = prev_row;
            st.anim_start = Instant::now();
            if !was_visible {
                st.pal = palette();
                st.shown_at = Instant::now();
                let border = st.pal.border;
                let _ = DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_BORDER_COLOR,
                    &border as *const _ as *const _,
                    4,
                );
            }
            let height = PAD * 2.0 + ROW_H * st.page_len as f32 + FOOTER_H;
            st.visible = true;
            ((st.width * st.scale) as i32, (height * st.scale) as i32, (PAD + INDEX_W) * st.scale, was_visible)
        });
        let Some((w, h, text_x, was_visible)) = geom else { return };
        unsafe {
            // Align candidate text with the composition text.
            let mut x = anchor.left - text_x as i32;
            let mut y = anchor.bottom + 4;
            let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
            let mon = MonitorFromPoint(POINT { x: anchor.left, y: anchor.bottom }, MONITOR_DEFAULTTONEAREST);
            if GetMonitorInfoW(mon, &mut mi).as_bool() {
                let wa = mi.rcWork;
                if y + h > wa.bottom {
                    y = anchor.top - h - 4;
                }
                x = x.clamp(wa.left, (wa.right - w).max(wa.left));
                y = y.max(wa.top);
            }
            if !was_visible {
                let _ = SetLayeredWindowAttributes(self.hwnd, COLORREF(0), 0, LWA_ALPHA);
            }
            let _ = SetWindowPos(self.hwnd, Some(HWND_TOPMOST), x, y, w, h, SWP_NOACTIVATE | SWP_SHOWWINDOW);
            SetTimer(Some(self.hwnd), TIMER_ID, 8, None);
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    /// Creates the Direct2D render target (and its D3D device) ahead of
    /// time; doing it on first show costs ~200 ms.
    /// Sets the handler for clicks on candidate rows.
    pub fn set_on_click(&self, f: impl Fn(usize) + 'static) {
        with_state(self.hwnd, |st| st.on_click = Some(std::rc::Rc::new(f)));
    }

    /// Sets the handler for background live conversion results.
    pub fn set_on_live(&self, f: impl Fn(String, String) + 'static) {
        with_state(self.hwnd, |st| st.on_live = Some(std::rc::Rc::new(f)));
    }

    /// Raw window handle, for [`post_live`] from other threads.
    pub fn raw(&self) -> isize {
        self.hwnd.0 as isize
    }

    /// Runs from the message loop so activation itself stays fast.
    pub fn warm_up(&self) {
        unsafe {
            let _ = PostMessageW(Some(self.hwnd), WM_APP_WARM_UP, WPARAM(0), LPARAM(0));
        }
    }

    pub fn hide(&self) {
        if with_state(self.hwnd, |st| std::mem::replace(&mut st.visible, false)) == Some(true) {
            unsafe {
                let _ = KillTimer(Some(self.hwnd), TIMER_ID);
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
        }
    }
}

impl Drop for CandidateWindow {
    fn drop(&mut self) {
        STATES.with(|s| s.borrow_mut().retain(|(h, _)| *h != self.hwnd.0 as isize));
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

fn render(hwnd: HWND, st: &mut State) -> Result<()> {
    unsafe {
        let mut rc = RECT::default();
        let _ = windows::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &mut rc);
        let size = D2D_SIZE_U { width: (rc.right - rc.left) as u32, height: (rc.bottom - rc.top) as u32 };
        if st.target.is_none() {
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_UNKNOWN },
                ..Default::default()
            };
            let hprops = D2D1_HWND_RENDER_TARGET_PROPERTIES { hwnd, pixelSize: size, presentOptions: D2D1_PRESENT_OPTIONS_IMMEDIATELY };
            st.target = Some(st.d2d.CreateHwndRenderTarget(&props, &hprops)?);
        }
        let rt = st.target.clone().unwrap();
        rt.Resize(&size)?;
        let dpi = 96.0 * st.scale;
        rt.SetDpi(dpi, dpi);
        rt.BeginDraw();
        rt.Clear(Some(&st.pal.bg));
        let brush = rt.CreateSolidColorBrush(&st.pal.text, None)?;
        let sub = rt.CreateSolidColorBrush(&st.pal.sub, None)?;
        let hover = rt.CreateSolidColorBrush(&st.pal.hover, None)?;
        let accent = rt.CreateSolidColorBrush(&st.pal.accent, None)?;

        // Selection pill + accent bar (animated).
        let row = st.highlight_row();
        if !st.suggest {
        let top = PAD + row * ROW_H;
        let pill = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: PAD, top: top + 2.0, right: st.width - PAD, bottom: top + ROW_H - 2.0 },
            radiusX: 4.0,
            radiusY: 4.0,
        };
        rt.FillRoundedRectangle(&pill, &hover);
        let bar = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: PAD, top: top + 9.0, right: PAD + 3.0, bottom: top + ROW_H - 9.0 },
            radiusX: 1.5,
            radiusY: 1.5,
        };
        rt.FillRoundedRectangle(&bar, &accent);
        }

        for (i, row) in st.rows.iter().enumerate() {
            let y = PAD + i as f32 * ROW_H;
            // Digits are typed as text while composing, so suggestions
            // are not numbered.
            if !st.suggest
                && let Ok(l) = st.dwrite.CreateTextLayout(
                    &wide(&(i + 1).to_string()),
                    &st.small,
                    INDEX_W,
                    ROW_H,
                )
            {
                let mut m = DWRITE_TEXT_METRICS::default();
                let _ = l.GetMetrics(&mut m);
                let origin = windows_numerics::Vector2 { X: PAD + 12.0, Y: y + (ROW_H - m.height) / 2.0 };
                rt.DrawTextLayout(origin, &l, &sub, Default::default());
            }
            if let Some((layout, height)) = row {
                let origin = windows_numerics::Vector2 {
                    X: PAD + INDEX_W,
                    Y: y + (ROW_H - height) / 2.0,
                };
                rt.DrawTextLayout(
                    origin,
                    layout,
                    &brush,
                    D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
                );
            }
        }
        // Footer: position / total.
        let footer = wide(&if st.suggest {
            "Tab".to_string()
        } else {
            format!("{} / {}", st.selected + 1, st.total)
        });
        if let Ok(l) = st.dwrite.CreateTextLayout(&footer, &st.small, 500.0, FOOTER_H) {
            let mut m = DWRITE_TEXT_METRICS::default();
            let _ = l.GetMetrics(&mut m);
            let y = PAD + st.page_len as f32 * ROW_H;
            let origin = windows_numerics::Vector2 {
                X: st.width - PAD - 10.0 - m.width,
                Y: y + (FOOTER_H - m.height) / 2.0 - 2.0,
            };
            rt.DrawTextLayout(origin, &l, &sub, Default::default());
        }
        if let Err(e) = rt.EndDraw(None, None) {
            // Device lost: recreate next time.
            st.target = None;
            return Err(e);
        }
        // Fade-in.
        let t = (st.shown_at.elapsed().as_secs_f32() * 1000.0 / FADE_MS).min(1.0);
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), (255.0 * t) as u8, LWA_ALPHA);
    }
    Ok(())
}

const WM_APP_WARM_UP: u32 = WM_APP + 1;
const WM_APP_LIVE: u32 = WM_APP + 2;

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                BeginPaint(hwnd, &mut ps);
                if GetWindowLongPtrW(hwnd, GWLP_USERDATA) != 0 {
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        with_state(hwnd, |st| render(hwnd, st));
                    }));
                }
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
                // Look up the row and handler, then release the state borrow
                // before calling out (the handler updates this window).
                let hit = with_state(hwnd, |st| {
                    let row = ((y / st.scale - PAD) / ROW_H).floor();
                    if st.visible && row >= 0.0 && (row as usize) < st.page_len {
                        st.on_click.clone().map(|f| (f, st.page_start + row as usize))
                    } else {
                        None
                    }
                })
                .flatten();
                if let Some((f, index)) = hit {
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(index)));
                }
                LRESULT(0)
            }
            WM_APP_LIVE => {
                let item = {
                    let mut m = LIVE_MAILBOX.lock().unwrap_or_else(|e| e.into_inner());
                    m.iter().position(|(h, _, _)| *h == hwnd.0 as isize).map(|i| m.remove(i))
                };
                let handler = with_state(hwnd, |st| st.on_live.clone()).flatten();
                if let (Some((_, reading, surface)), Some(f)) = (item, handler) {
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(reading, surface)));
                }
                LRESULT(0)
            }
            WM_APP_WARM_UP => {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    with_state(hwnd, |st| {
                        if st.target.is_none() {
                            let _ = render(hwnd, st);
                        }
                    });
                }));
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == TIMER_ID => {
                let animating = with_state(hwnd, |st| st.visible && st.animating()).unwrap_or(false);
                let _ = InvalidateRect(Some(hwnd), None, false);
                if !animating {
                    let _ = KillTimer(Some(hwnd), TIMER_ID);
                }
                let _ = ValidateRect(Some(hwnd), None);
                with_state(hwnd, |st| render(hwnd, st));
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}
