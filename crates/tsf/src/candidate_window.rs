//! Fluent-style candidate window drawn with Direct2D / DirectWrite.
//!
//! The window never takes activation; it is a top-most tool popup with
//! DWM rounded corners, follows the configured theme (by default the system light/dark
//! theme and accent color), fades in and animates the selection highlight.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Once;
use std::time::Instant;

use sweet_engine::theme::{AcrylicBlur, Rgb};
use sweet_engine::{CandidateList, ListKind};
use windows::Win32::Foundation::{
    COLORREF, E_FAIL, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM,
};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D_SIZE_U, D2D1_ALPHA_MODE_IGNORE, D2D1_ALPHA_MODE_PREMULTIPLIED,
    D2D1_ALPHA_MODE_UNKNOWN, D2D1_COLOR_F, D2D1_GRADIENT_STOP, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_DRAW_TEXT_OPTIONS_CLIP,
    D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT, D2D1_EXTEND_MODE_WRAP,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_GAMMA_2_2, D2D1_HWND_RENDER_TARGET_PROPERTIES,
    D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES, D2D1_PRESENT_OPTIONS_IMMEDIATELY,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_SOFTWARE, D2D1_ROUNDED_RECT,
    D2D1CreateFactory, ID2D1DCRenderTarget, ID2D1Factory, ID2D1HwndRenderTarget,
    ID2D1LinearGradientBrush, ID2D1RenderTarget, ID2D1StrokeStyle,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_METRICS, DWRITE_TRIMMING,
    DWRITE_TRIMMING_GRANULARITY_CHARACTER, DWRITE_WORD_WRAPPING_NO_WRAP, DWriteCreateFactory,
    IDWriteFactory, IDWriteFontCollection, IDWriteTextFormat, IDWriteTextLayout,
};
use windows::Win32::Graphics::Dwm::{
    DWMWA_BORDER_COLOR, DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE,
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmExtendFrameIntoClientArea,
    DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, BeginPaint,
    CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, EndPaint,
    GetMonitorInfoW, HBITMAP, HDC, HGDIOBJ, InvalidateRect, MONITOR_DEFAULTTONEAREST, MONITORINFO,
    MonitorFromPoint, PAINTSTRUCT, SelectObject, ValidateRect,
};
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, RRF_RT_REG_BINARY, RRF_RT_REG_DWORD, RegGetValueW,
};
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CS_DROPSHADOW, CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow, GA_ROOT,
    GWL_EXSTYLE, GWLP_HWNDPARENT, GWLP_USERDATA, GetAncestor, GetWindowLongPtrW, GetWindowLongW,
    HTTRANSPARENT, HWND_TOPMOST, IDC_ARROW, KillTimer, LWA_ALPHA, LoadCursorW, MA_NOACTIVATE,
    PostMessageW, RegisterClassExW, SW_HIDE, SWP_NOACTIVATE, SWP_NOREDRAW, SWP_NOZORDER,
    SWP_SHOWWINDOW, SetLayeredWindowAttributes, SetTimer, SetWindowLongPtrW, SetWindowLongW,
    SetWindowPos, ShowWindow, ULW_ALPHA, ULW_OPAQUE, UpdateLayeredWindow, WM_APP, WM_LBUTTONUP,
    WM_MOUSEACTIVATE, WM_NCHITTEST, WM_PAINT, WM_TIMER, WNDCLASSEXW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows_core::{Error, HSTRING, Interface, PCWSTR, Result, w};

use crate::acrylic::{BlurComposition, BlurQueue};

const CLASS_NAME: PCWSTR = w!("SweetImeCandidateWindow");
const BACKDROP_CLASS: PCWSTR = w!("SweetImeCandidateBackdrop");
const TIMER_ID: usize = 1;
/// Delayed hide, so that a list that disappears for a keystroke or two
/// while typing doesn't blink.
const HIDE_TIMER_ID: usize = 2;
const HIDE_DELAY_MS: u32 = 160;
/// Another look for the composition's position (see [`relayout_after`]).
const RELAYOUT_TIMER_ID: usize = 3;
const RAINBOW_TIMER_ID: usize = 4;
const RAINBOW_FRAME_MS: u32 = 33;
/// Showing again this soon after hiding skips the fade-in.
const REFADE_AFTER_MS: u128 = 600;

// Layout in DIPs.
const PAD: f32 = 4.0;
const GAMING_PAD: f32 = 6.0;
const ROW_H: f32 = 32.0;
const INDEX_W: f32 = 28.0;
const TEXT_PAD_R: f32 = 16.0;
const FOOTER_H: f32 = 24.0;
const INPUT_H: f32 = 22.0;
const INPUT_PAD: f32 = 12.0;
const INPUT_MAX_W: f32 = 360.0;
const FONT_SIZE: f32 = 16.0;
const MIN_W: f32 = 140.0;
const FADE_MS: f32 = 90.0;
const FADE_OUT_MS: f32 = 90.0;
const SLIDE_MS: f32 = 70.0;
const RESIZE_MS: f32 = 110.0;

fn candidate_format(dwrite: &IDWriteFactory, family: &str) -> Result<IDWriteTextFormat> {
    let locale = w!("ja-jp");
    let create = |name: PCWSTR, collection: Option<&IDWriteFontCollection>| unsafe {
        dwrite.CreateTextFormat(
            name,
            collection,
            DWRITE_FONT_WEIGHT_NORMAL,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            FONT_SIZE,
            locale,
        )
    };
    if !family.is_empty() {
        let name = HSTRING::from(family);
        let mut collection = None;
        unsafe {
            if dwrite
                .GetSystemFontCollection(&mut collection, true)
                .is_ok()
                && let Some(collection) = collection.as_ref()
            {
                let mut index = 0;
                let mut exists = false.into();
                if collection
                    .FindFamilyName(&name, &mut index, &mut exists)
                    .is_ok()
                    && exists.as_bool()
                    && let Ok(format) = create(PCWSTR(name.as_ptr()), Some(collection))
                {
                    return Ok(format);
                }
            }
        }
    }
    create(w!("Yu Gothic UI"), None)
}

#[derive(Clone, Copy)]
struct Palette {
    bg: D2D1_COLOR_F,
    bg_alpha: f32,
    acrylic: Option<(bool, AcrylicBlur)>,
    text: D2D1_COLOR_F,
    sub: D2D1_COLOR_F,
    hover: D2D1_COLOR_F,
    accent: D2D1_COLOR_F,
    border: COLORREF,
}

fn rgb(r: u8, g: u8, b: u8, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a,
    }
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

fn windows_accent(dark: bool) -> Option<D2D1_COLOR_F> {
    // AccentPalette stores 8 RGBA entries (Light3, Light2, Light1, Base, Dark1, ...).
    // Windows 11 uses Light2 (index 1) on dark surfaces and Dark1 (index 4) on light surfaces.
    let mut palette = [0u8; 32];
    let mut size = palette.len() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Accent"),
            w!("AccentPalette"),
            RRF_RT_REG_BINARY,
            None,
            Some(palette.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if status.is_ok() && size >= 20 {
        let offset = if dark { 4 } else { 16 };
        return Some(rgb(
            palette[offset],
            palette[offset + 1],
            palette[offset + 2],
            1.0,
        ));
    }

    // Fallback to DWM AccentColor (stored as 0xAABBGGRR).
    let v = read_dword(w!("Software\\Microsoft\\Windows\\DWM"), w!("AccentColor"))?;
    let (r, g, b) = (
        (v & 0xff) as u8,
        ((v >> 8) & 0xff) as u8,
        ((v >> 16) & 0xff) as u8,
    );
    Some(if dark {
        let l = |c: u8| c + ((255 - c) as f32 * 0.45) as u8;
        rgb(l(r), l(g), l(b), 1.0)
    } else {
        rgb(r, g, b, 1.0)
    })
}

fn palette() -> Palette {
    let theme = crate::theme();
    let custom = (theme == sweet_engine::Theme::Custom).then(sweet_engine::paths::load_user_theme);
    let c = custom.map_or_else(|| theme.colors(system_light()), |theme| theme.colors());
    let col = |v: Rgb, a: f32| rgb(v.0, v.1, v.2, a);
    let accent = match c.accent {
        Some(v) => col(v, 1.0),
        None => windows_accent(c.dark).unwrap_or_else(|| {
            if c.dark {
                rgb(0x60, 0xcd, 0xff, 1.0)
            } else {
                rgb(0x00, 0x5f, 0xb8, 1.0)
            }
        }),
    };
    let b = c.border;
    Palette {
        bg: col(c.bg, 1.0),
        bg_alpha: c.bg_alpha.clamp(sweet_engine::theme::MIN_BG_ALPHA, 1.0),
        acrylic: custom
            .filter(|theme| theme.acrylic && c.bg_alpha < 1.0)
            .map(|theme| (c.dark, theme.acrylic_blur)),
        text: col(c.text, 1.0),
        sub: col(c.sub, 1.0),
        hover: col(c.hover, c.hover_alpha),
        accent,
        // COLORREF is 0x00BBGGRR.
        border: COLORREF(u32::from(b.0) | u32::from(b.1) << 8 | u32::from(b.2) << 16),
    }
}

struct State {
    backdrop: Option<Backdrop>,
    d2d: ID2D1Factory,
    dwrite: IDWriteFactory,
    target: Option<RenderBackend>,
    bitmap_popup: bool,
    per_pixel_alpha: bool,
    format: IDWriteTextFormat,
    font_family: String,
    small: IDWriteTextFormat,
    pal: Palette,
    gaming_mode: bool,
    fade_enabled: bool,
    rainbow_border: Option<RainbowBorder>,
    items: Vec<String>,
    rows: Vec<Option<(IDWriteTextLayout, f32)>>,
    input: Option<(IDWriteTextLayout, DWRITE_TEXT_METRICS)>,
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
    /// Animated window size (logical DIPs) and animation start.
    size_from: (f32, f32),
    size_to: (f32, f32),
    size_anim_start: Instant,
    placement: Option<Placement>,
    /// Last window rect applied via SetWindowPos (x, y, w, h in physical px).
    last_rect: Option<(i32, i32, i32, i32)>,
    shown_at: Instant,
    hidden_at: Option<Instant>,
    fading_out: Option<(Instant, f32)>,
    visible: bool,
    /// Called with the absolute candidate index when a row is clicked.
    on_click: Option<std::rc::Rc<dyn Fn(usize)>>,
    /// Called with (reading, surface) when a background live conversion
    /// result arrives.
    on_live: Option<std::rc::Rc<dyn Fn(BackgroundResult)>>,
    /// Called when a [`relayout_after`] timer fires.
    on_relayout: Option<std::rc::Rc<dyn Fn()>>,
    /// Where the list was last shown (screen rectangle of the text).
    last_anchor: Option<RECT>,
}

/// Calls the window's relayout handler after `ms` (from the window's
/// thread), to look for the composition's position again.
pub fn relayout_after(hwnd: isize, ms: u32) {
    unsafe {
        SetTimer(Some(HWND(hwnd as *mut _)), RELAYOUT_TIMER_ID, ms, None);
    }
}

/// Results from the live-conversion worker, keyed by window handle.
pub enum BackgroundResult {
    Live(String, String),
    Simeji(sweet_engine::simeji::Response),
}

static LIVE_MAILBOX: std::sync::Mutex<Vec<(isize, BackgroundResult)>> =
    std::sync::Mutex::new(Vec::new());

/// Delivers a live conversion result to the window `hwnd` (any thread).
pub fn post_live(hwnd: isize, reading: String, surface: String) {
    post_result(hwnd, BackgroundResult::Live(reading, surface));
}

pub fn post_result(hwnd: isize, result: BackgroundResult) {
    let mut m = LIVE_MAILBOX.lock().unwrap_or_else(|e| e.into_inner());
    let same_kind =
        |item: &BackgroundResult| std::mem::discriminant(item) == std::mem::discriminant(&result);
    if m.iter().any(|(h, item)| {
        *h == hwnd && matches!((item, &result), (BackgroundResult::Simeji(old), BackgroundResult::Simeji(new)) if old.id > new.id)
    }) {
        return;
    }
    m.retain(|(h, item)| *h != hwnd || !same_kind(item));
    unsafe {
        if PostMessageW(
            Some(HWND(hwnd as *mut _)),
            WM_APP_LIVE,
            WPARAM(0),
            LPARAM(0),
        )
        .is_ok()
        {
            m.push((hwnd, result));
        }
    }
}

impl State {
    fn padding(&self) -> f32 {
        PAD + if self.gaming_mode { GAMING_PAD } else { 0.0 }
    }

    fn window_width(&self) -> f32 {
        let input_width = self.input.as_ref().map_or(0.0, |(_, m)| {
            (m.widthIncludingTrailingWhitespace.min(INPUT_MAX_W) + 2.0 * (PAD + INPUT_PAD)).ceil()
        });
        self.width.max(input_width) + 2.0 * (self.padding() - PAD)
    }

    fn rows_top(&self) -> f32 {
        self.padding() + if self.input.is_some() { INPUT_H } else { 0.0 }
    }

    fn target_height(&self) -> f32 {
        let padding = self.padding();
        self.rows_top()
            + padding
            + if self.page_len > 0 {
                ROW_H * self.page_len as f32 + FOOTER_H
            } else {
                0.0
            }
    }

    fn animated_size(&self) -> (f32, f32) {
        interpolate_size(
            self.size_from,
            self.size_to,
            self.size_anim_start.elapsed().as_secs_f32() * 1000.0,
        )
    }

    fn is_resizing(&self) -> bool {
        let dx = (self.size_to.0 - self.size_from.0).abs();
        let dy = (self.size_to.1 - self.size_from.1).abs();
        (dx > 0.5 || dy > 0.5) && self.size_anim_start.elapsed().as_secs_f32() * 1000.0 < RESIZE_MS
    }

    fn highlight_row(&self) -> f32 {
        let target = (self.selected - self.page_start) as f32;
        let t = (self.anim_start.elapsed().as_secs_f32() * 1000.0 / SLIDE_MS).min(1.0);
        let e = 1.0 - (1.0 - t).powi(3); // ease-out cubic
        self.anim_from + (target - self.anim_from) * e
    }

    fn fade_factor(&self) -> f32 {
        if let Some((start, from)) = self.fading_out {
            let t = (start.elapsed().as_secs_f32() * 1000.0 / FADE_OUT_MS).min(1.0);
            let e = 1.0 - (1.0 - t).powi(3);
            return (from * (1.0 - e)).clamp(0.0, 1.0);
        }
        if !self.fade_enabled {
            return 1.0;
        }
        let t = (self.shown_at.elapsed().as_secs_f32() * 1000.0 / FADE_MS).min(1.0);
        1.0 - (1.0 - t).powi(3)
    }

    fn animating(&self) -> bool {
        if self.fading_out.is_some() {
            return true;
        }
        self.is_resizing()
            || self.anim_start.elapsed().as_secs_f32() * 1000.0 < SLIDE_MS
            || (self.fade_enabled && self.shown_at.elapsed().as_secs_f32() * 1000.0 < FADE_MS)
    }
}

fn interpolate_size(from: (f32, f32), to: (f32, f32), elapsed_ms: f32) -> (f32, f32) {
    let t = (elapsed_ms / RESIZE_MS).clamp(0.0, 1.0);
    let e = 1.0 - (1.0 - t).powi(3); // ease-out cubic
    (from.0 + (to.0 - from.0) * e, from.1 + (to.1 - from.1) * e)
}

#[derive(Clone, Copy)]
struct Placement {
    x: i32,
    edge: i32,
    above: bool,
    work_top: i32,
}

impl Placement {
    fn rect(self, w: i32, h: i32) -> (i32, i32, i32, i32) {
        let y = if self.above { self.edge - h } else { self.edge };
        (self.x, y.max(self.work_top), w, h)
    }
}

fn choose_placement(
    anchor: RECT,
    work: RECT,
    w: i32,
    h: i32,
    text_x: f32,
    previous: Option<Placement>,
) -> Placement {
    let below = work.bottom - anchor.bottom - 4;
    let above = anchor.top - work.top - 4;
    let place_above = match previous {
        Some(p) if p.above && (above >= h || below < h) => true,
        Some(p) if !p.above && (below >= h || above < h) => false,
        _ => below < h && (above >= h || above > below),
    };
    let aligned_x = anchor.left - text_x as i32;
    // Beside the right edge, growing may need more room to the left. Keep
    // that position while the anchor stays put and the popup shrinks again.
    let x = previous.map_or(aligned_x, |p| p.x.min(aligned_x));
    Placement {
        x: x.clamp(work.left, (work.right - w).max(work.left)),
        edge: if place_above { anchor.top - 4 } else { anchor.bottom + 4 },
        above: place_above,
        work_top: work.top,
    }
}

unsafe fn monitor_work_area(anchor: RECT) -> RECT {
    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let mon = unsafe {
        MonitorFromPoint(
            POINT {
                x: anchor.left,
                y: anchor.bottom,
            },
            MONITOR_DEFAULTTONEAREST,
        )
    };
    if unsafe { GetMonitorInfoW(mon, &mut mi).as_bool() } {
        return mi.rcWork;
    }
    // An unavailable work area should not constrain a valid host anchor.
    RECT { left: i32::MIN / 2, top: i32::MIN / 2, right: i32::MAX / 2, bottom: i32::MAX / 2 }
}

thread_local! {
    // The window lives on the host's UI thread; state is per-thread.
    static STATES: RefCell<Vec<(isize, Box<State>)>> = const { RefCell::new(Vec::new()) };
}

fn with_state<R>(hwnd: HWND, f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATES.with(|s| {
        let mut s = s.borrow_mut();
        s.iter_mut()
            .find(|(h, _)| *h == hwnd.0 as isize)
            .map(|(_, st)| f(st))
    })
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

pub struct CandidateWindow {
    hwnd: HWND,
    owner: std::cell::Cell<isize>,
    blur_queue: RefCell<Option<Rc<BlurQueue>>>,
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
        RegisterClassExW(&WNDCLASSEXW {
            cbSize: wc.cbSize,
            lpfnWndProc: Some(backdrop_proc),
            hInstance: wc.hInstance,
            lpszClassName: BACKDROP_CLASS,
            ..Default::default()
        });
    });
}

unsafe extern "system" fn backdrop_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

// DWM backdrops require a redirected window. Keep the material directly below
// the layered candidate surface, which is also used in immersive hosts.
struct Backdrop {
    hwnd: HWND,
    appearance: (bool, AcrylicBlur),
    composition: Option<BlurComposition>,
}

impl Drop for Backdrop {
    fn drop(&mut self) {
        drop(self.composition.take());
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

fn create_backdrop(owner: Option<HWND>, dark: bool, blur: AcrylicBlur) -> Result<Backdrop> {
    unsafe {
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            BACKDROP_CLASS,
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
        let margins = MARGINS {
            cxLeftWidth: -1,
            cxRightWidth: -1,
            cyTopHeight: -1,
            cyBottomHeight: -1,
        };
        let dark_mode = i32::from(dark);
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            (&raw const dark_mode).cast(),
            4,
        );
        let corner = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            (&raw const corner).cast(),
            4,
        );
        let mut backdrop = Backdrop {
            hwnd,
            appearance: (dark, blur),
            composition: None,
        };
        DwmExtendFrameIntoClientArea(hwnd, &margins)?;
        let deviation = match blur {
            AcrylicBlur::Fine => 4.0,
            AcrylicBlur::Standard => 24.0,
            AcrylicBlur::Coarse => 48.0,
        };
        match crate::acrylic::custom_blur(hwnd, deviation) {
            Ok(composition) => {
                backdrop.composition = Some(composition);
                return Ok(backdrop);
            }
            Err(error) => crate::debug_log!("custom acrylic blur: {error}"),
        }
        // DWMSBT_TRANSIENTWINDOW selects Desktop Acrylic on Windows 11.
        let acrylic = 3i32;
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE,
            (&raw const acrylic).cast(),
            4,
        )?;
        Ok(backdrop)
    }
}

fn position_backdrop(hwnd: HWND, (x, y, w, h): (i32, i32, i32, i32)) {
    if let Some(backdrop) = with_state(hwnd, |st| st.backdrop.as_ref().map(|b| b.hwnd)).flatten() {
        unsafe {
            if SetWindowPos(
                backdrop,
                Some(hwnd),
                x,
                y,
                w,
                h,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            )
            .is_err()
            {
                let _ = ShowWindow(backdrop, SW_HIDE);
            }
        }
    }
}

fn destroy_backdrop(hwnd: HWND) {
    drop(with_state(hwnd, |st| st.backdrop.take()));
}

fn root_owner(owner: HWND) -> HWND {
    let root = unsafe { GetAncestor(owner, GA_ROOT) };
    if root.is_invalid() { owner } else { root }
}

impl CandidateWindow {
    /// Creates the window owned by `owner` (the application window the
    /// text is in). Owned windows stay above their owner, including in
    /// immersive hosts such as Windows Search, where an unowned topmost
    /// window would end up behind the host's UI.
    pub fn new(owner: Option<HWND>, bitmap_popup: bool) -> Result<Self> {
        register_class();
        let owner = owner.map(root_owner);
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
            let locale = HSTRING::from("ja-jp");
            let format = candidate_format(&dwrite, "")?;
            let small = dwrite.CreateTextFormat(
                &HSTRING::from("Segoe UI Variable Text"),
                None,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                12.0,
                &locale,
            )?;
            let state = State {
                backdrop: None,
                d2d,
                dwrite,
                target: None,
                bitmap_popup,
                per_pixel_alpha: false,
                format,
                font_family: String::new(),
                small,
                pal: palette(),
                gaming_mode: false,
                fade_enabled: true,
                rainbow_border: None,
                items: Vec::new(),
                rows: Vec::new(),
                input: None,
                selected: 0,
                page_start: 0,
                page_len: 0,
                total: 0,
                suggest: false,
                scale: 1.0,
                width: MIN_W,
                anim_from: 0.0,
                anim_start: Instant::now(),
                size_from: (MIN_W, 0.0),
                size_to: (MIN_W, 0.0),
                size_anim_start: Instant::now(),
                placement: None,
                last_rect: None,
                shown_at: Instant::now(),
                hidden_at: None,
                fading_out: None,
                visible: false,
                on_click: None,
                on_live: None,
                on_relayout: None,
                last_anchor: None,
            };
            STATES.with(|s| s.borrow_mut().push((hwnd.0 as isize, Box::new(state))));
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 1);
            Ok(Self {
                hwnd,
                owner: std::cell::Cell::new(owner.map_or(0, |h| h.0 as isize)),
                blur_queue: RefCell::new(None),
            })
        }
    }

    /// Changes the owner window (e.g. focus moved to another window of the
    /// same thread).
    pub fn set_owner(&self, owner: HWND) {
        let owner = root_owner(owner);
        if self.owner.get() != owner.0 as isize {
            self.owner.set(owner.0 as isize);
            with_state(self.hwnd, |st| st.placement = None);
            unsafe {
                SetWindowLongPtrW(self.hwnd, GWLP_HWNDPARENT, owner.0 as _);
                if let Some(backdrop) =
                    with_state(self.hwnd, |st| st.backdrop.as_ref().map(|b| b.hwnd)).flatten()
                {
                    SetWindowLongPtrW(backdrop, GWLP_HWNDPARENT, owner.0 as _);
                }
            }
        }
    }

    /// Shows the candidate list and/or typed input next to the focused text.
    pub fn show(
        &self,
        list: Option<&CandidateList>,
        input: Option<&str>,
        anchor: RECT,
        gaming_mode: bool,
        window_fade: bool,
        font_family: &str,
    ) {
        let gaming_mode = gaming_mode && list.is_some();
        let hwnd = self.hwnd;
        unsafe {
            let _ = KillTimer(Some(hwnd), RELAYOUT_TIMER_ID);
        }
        let geom = with_state(hwnd, |st| unsafe {
            let previous = st.placement.filter(|_| {
                st.visible && st.last_anchor.is_some_and(|last| {
                    (last.left, last.top, last.bottom) == (anchor.left, anchor.top, anchor.bottom)
                })
            });
            st.last_anchor = Some(anchor);
            let dpi = GetDpiForWindow(hwnd);
            st.scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
            let (page_start, items, selected, total, suggest) =
                list.map_or((0, &[][..], 0, 0, false), |list| {
                    let start = list.page_start();
                    let end = (start + list.page_size).min(list.items.len());
                    (
                        start,
                        &list.items[start..end],
                        list.selected,
                        list.items.len(),
                        list.kind == ListKind::Suggestion,
                    )
                });
            let was_visible = st.visible;
            let prev_size = if was_visible {
                st.animated_size()
            } else {
                (0.0, 0.0)
            };
            if !was_visible || st.gaming_mode != gaming_mode {
                if gaming_mode {
                    SetTimer(Some(hwnd), RAINBOW_TIMER_ID, RAINBOW_FRAME_MS, None);
                } else {
                    let _ = KillTimer(Some(hwnd), RAINBOW_TIMER_ID);
                    st.rainbow_border = None;
                }
                st.gaming_mode = gaming_mode;
            }
            let prev_row = if was_visible && st.page_len > 0 && st.page_start == page_start {
                st.highlight_row()
            } else {
                (selected - page_start) as f32
            };
            let mut font_changed = false;
            if st.font_family != font_family
                && let Ok(format) = candidate_format(&st.dwrite, font_family)
            {
                st.font_family = font_family.to_owned();
                st.format = format;
                font_changed = true;
            }
            if font_changed || st.items != items || st.rows.iter().any(Option::is_none) {
                let prev_width = st.width;
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
                // Predictions change on every keystroke: don't let the
                // window shrink and grow with them.
                if !font_changed && was_visible && suggest && st.suggest {
                    st.width = st.width.max(prev_width);
                }
            }
            st.input = input.and_then(|text| {
                let layout = st
                    .dwrite
                    .CreateTextLayout(&wide(text), &st.small, INPUT_MAX_W, INPUT_H)
                    .ok()?;
                layout.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP).ok()?;
                let ellipsis = st.dwrite.CreateEllipsisTrimmingSign(&st.small).ok()?;
                let trim = DWRITE_TRIMMING {
                    granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                    ..Default::default()
                };
                layout.SetTrimming(&trim, &ellipsis).ok()?;
                let mut metrics = DWRITE_TEXT_METRICS::default();
                layout.GetMetrics(&mut metrics).ok()?;
                Some((layout, metrics))
            });
            st.page_start = page_start;
            st.page_len = items.len();
            st.selected = selected;
            st.total = total;
            st.suggest = suggest;
            st.anim_from = prev_row;
            st.anim_start = Instant::now();
            st.fade_enabled = window_fade;
            let was_fading_out = st.fading_out.take().is_some();
            let mut fade = false;
            if !was_visible {
                st.pal = palette();
                fade = window_fade
                    && !was_fading_out
                    && st
                        .hidden_at
                        .is_none_or(|t| t.elapsed().as_millis() > REFADE_AFTER_MS);
                st.shown_at = if fade {
                    Instant::now()
                } else {
                    Instant::now()
                        .checked_sub(std::time::Duration::from_secs(1))
                        .unwrap_or_else(Instant::now)
                };
                let border = st.pal.border;
                let _ = DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_BORDER_COLOR,
                    &border as *const _ as *const _,
                    4,
                );
            }
            let padding = st.padding();
            let target_size = (st.window_width(), st.target_height());
            if !was_visible {
                st.size_from = target_size;
                st.size_to = target_size;
                st.size_anim_start = Instant::now()
                    .checked_sub(std::time::Duration::from_secs(1))
                    .unwrap_or_else(Instant::now);
            } else if (target_size.0 - st.size_to.0).abs() > 0.5
                || (target_size.1 - st.size_to.1).abs() > 0.5
            {
                st.size_from = prev_size;
                st.size_to = target_size;
                st.size_anim_start = Instant::now();
            }
            let (cur_w, cur_h) = st.animated_size();
            let text_offset = if st.page_len > 0 { INDEX_W } else { INPUT_PAD };
            let text_x = (padding + text_offset) * st.scale;
            st.visible = true;
            let w_px = (cur_w * st.scale).round().max(1.0) as i32;
            let h_px = (cur_h * st.scale).round().max(1.0) as i32;
            let placement = choose_placement(
                anchor,
                monitor_work_area(anchor),
                w_px.max((target_size.0 * st.scale).round() as i32),
                (target_size.1 * st.scale).round() as i32,
                text_x,
                previous,
            );
            st.placement = Some(placement);
            let rect = placement.rect(w_px, h_px);
            st.last_rect = Some(rect);
            (rect, was_visible, fade, st.bitmap_popup, st.pal)
        });
        let Some(((x, y, w, h), was_visible, fade, bitmap_popup, pal)) = geom else {
            return;
        };
        if !was_visible
            && with_state(self.hwnd, |st| st.backdrop.as_ref().map(|b| b.appearance))
                != Some(pal.acrylic)
        {
            destroy_backdrop(self.hwnd);
            if let Some((dark, blur)) = pal.acrylic {
                match BlurQueue::acquire() {
                    Ok(queue) => {
                        self.blur_queue.replace(queue);
                    }
                    Err(error) => {
                        crate::debug_log!("initialize acrylic dispatcher queue: {error}")
                    }
                }
                let owner = HWND(self.owner.get() as _);
                let backdrop =
                    create_backdrop((!owner.is_invalid()).then_some(owner), dark, blur).ok();
                with_state(self.hwnd, |st| st.backdrop = backdrop);
            }
        }
        let (per_pixel_alpha, changed) = with_state(self.hwnd, |st| {
            let per_pixel_alpha = st.backdrop.is_some();
            let changed = st.per_pixel_alpha != per_pixel_alpha;
            if changed {
                st.per_pixel_alpha = per_pixel_alpha;
                st.rainbow_border = None;
                st.target = None;
            }
            (per_pixel_alpha, changed)
        })
        .unwrap_or_default();
        unsafe {
            if changed && !bitmap_popup {
                // Switching between SetLayeredWindowAttributes and
                // UpdateLayeredWindow requires resetting WS_EX_LAYERED.
                let style = GetWindowLongW(self.hwnd, GWL_EXSTYLE);
                SetWindowLongW(self.hwnd, GWL_EXSTYLE, style & !(WS_EX_LAYERED.0 as i32));
                SetWindowLongW(self.hwnd, GWL_EXSTYLE, style);
            }
            let _ = KillTimer(Some(self.hwnd), HIDE_TIMER_ID);
            if !was_visible && !bitmap_popup && !per_pixel_alpha {
                let alpha = if fade {
                    0
                } else {
                    (255.0 * pal.bg_alpha).round() as u8
                };
                let _ = SetLayeredWindowAttributes(self.hwnd, COLORREF(0), alpha, LWA_ALPHA);
            }
            let _ = SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                x,
                y,
                w,
                h,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            position_backdrop(self.hwnd, (x, y, w, h));
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
    pub fn set_on_live(&self, f: impl Fn(BackgroundResult) + 'static) {
        with_state(self.hwnd, |st| st.on_live = Some(std::rc::Rc::new(f)));
    }

    /// Sets the handler for [`relayout_after`].
    pub fn set_on_relayout(&self, f: impl Fn() + 'static) {
        with_state(self.hwnd, |st| st.on_relayout = Some(std::rc::Rc::new(f)));
    }

    /// Where the list was last shown, if it is still showing.
    pub fn visible_anchor(&self) -> Option<RECT> {
        with_state(self.hwnd, |st| st.last_anchor.filter(|_| st.visible)).flatten()
    }

    /// Where the list was last shown.
    pub fn last_anchor(&self) -> Option<RECT> {
        with_state(self.hwnd, |st| st.last_anchor).flatten()
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
        hide_now(self.hwnd);
    }

    /// Hides after a short delay unless shown again first (while typing).
    pub fn hide_soon(&self) {
        if with_state(self.hwnd, |st| st.input.is_some()) == Some(true) {
            self.hide();
            return;
        }
        if with_state(self.hwnd, |st| st.visible) == Some(true) {
            unsafe {
                SetTimer(Some(self.hwnd), HIDE_TIMER_ID, HIDE_DELAY_MS, None);
            }
        }
    }
}

fn hide_now(hwnd: HWND) {
    let start_fade = with_state(hwnd, |st| {
        if !st.visible {
            return None;
        }
        let from = st.fade_factor();
        st.visible = false;
        if st.fade_enabled && from > 0.01 {
            st.fading_out = Some((Instant::now(), from));
            Some(true)
        } else {
            st.fading_out = None;
            st.hidden_at = Some(Instant::now());
            Some(false)
        }
    })
    .flatten();
    unsafe {
        let _ = KillTimer(Some(hwnd), HIDE_TIMER_ID);
        match start_fade {
            Some(true) => {
                let _ = KillTimer(Some(hwnd), RAINBOW_TIMER_ID);
                SetTimer(Some(hwnd), TIMER_ID, 8, None);
            }
            Some(false) => {
                finish_hide(hwnd);
            }
            None => {}
        }
    }
}

fn finish_hide(hwnd: HWND) {
    let backdrop = with_state(hwnd, |st| {
        st.visible = false;
        st.input = None;
        st.fading_out = None;
        st.last_rect = None;
        st.hidden_at = Some(Instant::now());
        st.backdrop.as_ref().map(|b| b.hwnd)
    })
    .flatten();
    unsafe {
        let _ = KillTimer(Some(hwnd), TIMER_ID);
        let _ = KillTimer(Some(hwnd), RAINBOW_TIMER_ID);
        if let Some(backdrop) = backdrop {
            let _ = ShowWindow(backdrop, SW_HIDE);
        }
        let _ = ShowWindow(hwnd, SW_HIDE);
    }
}

impl Drop for CandidateWindow {
    fn drop(&mut self) {
        destroy_backdrop(self.hwnd);
        STATES.with(|s| s.borrow_mut().retain(|(h, _)| *h != self.hwnd.0 as isize));
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
        LIVE_MAILBOX
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|(h, _)| *h != self.hwnd.0 as isize);
    }
}

struct RainbowBorder {
    colors: ID2D1LinearGradientBrush,
    glint: ID2D1LinearGradientBrush,
}

fn make_rainbow_border(rt: &ID2D1RenderTarget) -> Result<RainbowBorder> {
    // Match the first and last color so the repeating gradient has no seam.
    let colors = [
        (255, 20, 103),
        (255, 154, 0),
        (209, 255, 0),
        (0, 255, 150),
        (0, 190, 255),
        (156, 29, 255),
        (255, 20, 103),
    ];
    let stops: [_; 7] = std::array::from_fn(|i| {
        let (r, g, b) = colors[i];
        D2D1_GRADIENT_STOP {
            position: i as f32 / 6.0,
            color: rgb(r, g, b, 1.0),
        }
    });
    let glint = [
        (0.0, 0.0),
        (0.38, 0.0),
        (0.46, 0.3),
        (0.5, 1.0),
        (0.58, 0.0),
        (1.0, 0.0),
    ]
    .map(|(position, alpha)| D2D1_GRADIENT_STOP {
        position,
        color: rgb(255, 255, 255, alpha),
    });
    let gradient = |stops: &[D2D1_GRADIENT_STOP]| unsafe {
        let stops =
            rt.CreateGradientStopCollection(stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_WRAP)?;
        let properties = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
            endPoint: windows_numerics::Vector2 { X: 1.0, Y: 0.0 },
            ..Default::default()
        };
        rt.CreateLinearGradientBrush(&properties, None, &stops)
    };
    Ok(RainbowBorder {
        colors: gradient(&stops)?,
        glint: gradient(&glint)?,
    })
}

fn draw_rainbow_border(
    rt: &ID2D1RenderTarget,
    brushes: &RainbowBorder,
    width: f32,
    height: f32,
    elapsed: f32,
) {
    use windows_numerics::Vector2;

    let inset = 2.5;
    let border = D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F {
            left: inset,
            top: inset,
            right: width - inset,
            bottom: height - inset,
        },
        radiusX: 8.0,
        radiusY: 8.0,
    };
    let slide = |brush: &ID2D1LinearGradientBrush, travel: f32| unsafe {
        let phase = travel.fract();
        brush.SetStartPoint(Vector2 {
            X: width * phase,
            Y: height * phase,
        });
        brush.SetEndPoint(Vector2 {
            X: width * (1.0 + phase),
            Y: height * (1.0 + phase),
        });
    };
    slide(&brushes.colors, elapsed / 1.2);
    slide(&brushes.glint, -elapsed / 0.9);
    unsafe {
        let edge_width = 2.3;
        for (thickness, opacity) in [(14.0, 0.14), (9.0, 0.23), (5.0, 0.53), (edge_width, 1.0)] {
            brushes.colors.SetOpacity(opacity);
            rt.DrawRoundedRectangle(
                &border,
                &brushes.colors,
                thickness,
                None::<&ID2D1StrokeStyle>,
            );
        }
        rt.DrawRoundedRectangle(
            &border,
            &brushes.glint,
            edge_width,
            None::<&ID2D1StrokeStyle>,
        );
    }
}

enum RenderBackend {
    Window(ID2D1HwndRenderTarget),
    Bitmap(ID2D1DCRenderTarget, LayeredBitmap),
}

/// The Search compositor can leave an HWND render target blank. An owned
/// layered popup can receive the complete candidate image in one update.
struct LayeredBitmap {
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    size: D2D_SIZE_U,
}

impl LayeredBitmap {
    fn new(size: D2D_SIZE_U) -> Result<Self> {
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size.width as i32,
                biHeight: -(size.height as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        unsafe {
            let dc = CreateCompatibleDC(None);
            if dc.is_invalid() {
                return Err(Error::new(
                    E_FAIL,
                    "Could not create the candidate bitmap DC",
                ));
            }
            let mut bits = std::ptr::null_mut();
            let bitmap = match CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)
            {
                Ok(bitmap) => bitmap,
                Err(error) => {
                    let _ = DeleteDC(dc);
                    return Err(error);
                }
            };
            let previous = SelectObject(dc, HGDIOBJ(bitmap.0));
            if previous.is_invalid() {
                let _ = DeleteObject(HGDIOBJ(bitmap.0));
                let _ = DeleteDC(dc);
                return Err(Error::new(E_FAIL, "Could not select the candidate bitmap"));
            }
            Ok(Self {
                dc,
                bitmap,
                previous,
                size,
            })
        }
    }

    fn present(&self, hwnd: HWND, alpha: u8, per_pixel_alpha: bool) -> Result<()> {
        let size = SIZE {
            cx: self.size.width as i32,
            cy: self.size.height as i32,
        };
        unsafe {
            if per_pixel_alpha || alpha < 255 {
                let blend = BLENDFUNCTION {
                    BlendOp: AC_SRC_OVER as u8,
                    BlendFlags: 0,
                    SourceConstantAlpha: alpha,
                    AlphaFormat: if per_pixel_alpha {
                        AC_SRC_ALPHA as u8
                    } else {
                        0
                    },
                };
                UpdateLayeredWindow(
                    hwnd,
                    None,
                    None,
                    Some(&size),
                    Some(self.dc),
                    Some(&POINT::default()),
                    COLORREF(0),
                    Some(&blend),
                    ULW_ALPHA,
                )
            } else {
                UpdateLayeredWindow(
                    hwnd,
                    None,
                    None,
                    Some(&size),
                    Some(self.dc),
                    Some(&POINT::default()),
                    COLORREF(0),
                    None,
                    ULW_OPAQUE,
                )
            }
        }
    }
}

impl Drop for LayeredBitmap {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.previous);
            let _ = DeleteObject(HGDIOBJ(self.bitmap.0));
            let _ = DeleteDC(self.dc);
        }
    }
}

fn render(hwnd: HWND, st: &mut State) -> Result<()> {
    unsafe {
        let mut rc = RECT::default();
        windows::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &mut rc)?;
        let size = D2D_SIZE_U {
            width: (rc.right - rc.left) as u32,
            height: (rc.bottom - rc.top) as u32,
        };
        if size.width == 0 || size.height == 0 {
            return Ok(());
        }
        if matches!(&st.target, Some(RenderBackend::Bitmap(_, bitmap)) if bitmap.size != size) {
            st.rainbow_border = None;
            st.target = None;
        }
        if st.target.is_none() {
            let mut props = D2D1_RENDER_TARGET_PROPERTIES {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_UNKNOWN,
                },
                ..Default::default()
            };
            st.target = Some(if st.bitmap_popup || st.per_pixel_alpha {
                props.r#type = D2D1_RENDER_TARGET_TYPE_SOFTWARE;
                props.pixelFormat.alphaMode = if st.per_pixel_alpha {
                    D2D1_ALPHA_MODE_PREMULTIPLIED
                } else {
                    D2D1_ALPHA_MODE_IGNORE
                };
                let bitmap = LayeredBitmap::new(size)?;
                let target = st.d2d.CreateDCRenderTarget(&props)?;
                target.BindDC(bitmap.dc, &rc)?;
                RenderBackend::Bitmap(target, bitmap)
            } else {
                let hprops = D2D1_HWND_RENDER_TARGET_PROPERTIES {
                    hwnd,
                    pixelSize: size,
                    presentOptions: D2D1_PRESENT_OPTIONS_IMMEDIATELY,
                };
                RenderBackend::Window(st.d2d.CreateHwndRenderTarget(&props, &hprops)?)
            });
        }
        let rt: ID2D1RenderTarget = match st.target.as_ref().unwrap() {
            RenderBackend::Window(target) => {
                target.Resize(&size)?;
                target.cast()?
            }
            RenderBackend::Bitmap(target, _) => target.cast()?,
        };
        if st.gaming_mode && st.rainbow_border.is_none() {
            st.rainbow_border = make_rainbow_border(&rt).ok();
        }
        let dpi = 96.0 * st.scale;
        rt.SetDpi(dpi, dpi);
        rt.BeginDraw();
        let mut background = st.pal.bg;
        if st.per_pixel_alpha {
            background.a = st.pal.bg_alpha;
        }
        rt.Clear(Some(&background));
        let brush = rt.CreateSolidColorBrush(&st.pal.text, None)?;
        let sub = rt.CreateSolidColorBrush(&st.pal.sub, None)?;
        let hover = rt.CreateSolidColorBrush(&st.pal.hover, None)?;
        let accent = rt.CreateSolidColorBrush(&st.pal.accent, None)?;
        let pad = st.padding();
        let width = size.width as f32 / st.scale;
        let height = size.height as f32 / st.scale;
        let rows_top = st.rows_top();

        if let Some((layout, metrics)) = &st.input {
            let origin = windows_numerics::Vector2 {
                X: pad + INPUT_PAD,
                Y: pad + (INPUT_H - metrics.height) / 2.0,
            };
            rt.DrawTextLayout(
                origin,
                layout,
                &sub,
                D2D1_DRAW_TEXT_OPTIONS_CLIP | D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
            );
        }

        // Clip candidate rows above the footer so shrinking smoothly never overlaps it.
        let rows_bottom = (height - pad - FOOTER_H).max(rows_top);
        if st.page_len > 0 {
            rt.PushAxisAlignedClip(
                &D2D_RECT_F {
                    left: pad,
                    top: rows_top,
                    right: (width - pad).max(pad),
                    bottom: rows_bottom,
                },
                D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            );
        }

        // Selection pill + accent bar (animated).
        if st.page_len > 0 && !st.suggest {
            let top = rows_top + st.highlight_row() * ROW_H;
            let pill = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: pad,
                    top: top + 2.0,
                    right: (width - pad).max(pad),
                    bottom: top + ROW_H - 2.0,
                },
                radiusX: 4.0,
                radiusY: 4.0,
            };
            rt.FillRoundedRectangle(&pill, &hover);
            let bar = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: pad,
                    top: top + 9.0,
                    right: pad + 3.0,
                    bottom: top + ROW_H - 9.0,
                },
                radiusX: 1.5,
                radiusY: 1.5,
            };
            rt.FillRoundedRectangle(&bar, &accent);
        }

        for (i, row) in st.rows.iter().enumerate() {
            let y = rows_top + i as f32 * ROW_H;
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
                let origin = windows_numerics::Vector2 {
                    X: pad + 12.0,
                    Y: y + (ROW_H - m.height) / 2.0,
                };
                rt.DrawTextLayout(origin, &l, &sub, Default::default());
            }
            if let Some((layout, height)) = row {
                let origin = windows_numerics::Vector2 {
                    X: pad + INDEX_W,
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
        if st.page_len > 0 {
            rt.PopAxisAlignedClip();
        }
        // Footer: position / total.
        let footer = wide(&if st.suggest {
            "Tab".to_string()
        } else {
            format!("{} / {}", st.selected + 1, st.total)
        });
        if st.page_len > 0
            && let Ok(l) = st
                .dwrite
                .CreateTextLayout(&footer, &st.small, 500.0, FOOTER_H)
        {
            let mut m = DWRITE_TEXT_METRICS::default();
            let _ = l.GetMetrics(&mut m);
            let y = rows_bottom;
            let origin = windows_numerics::Vector2 {
                X: (width - pad - 10.0 - m.width).max(pad),
                Y: y + (FOOTER_H - m.height) / 2.0 - 2.0,
            };
            rt.DrawTextLayout(origin, &l, &sub, Default::default());
        }
        if let Some(brushes) = &st.rainbow_border {
            draw_rainbow_border(
                &rt,
                brushes,
                size.width as f32 / st.scale,
                size.height as f32 / st.scale,
                st.shown_at.elapsed().as_secs_f32(),
            );
        }
        if let Err(e) = rt.EndDraw(None, None) {
            // Device lost: recreate next time.
            st.target = None;
            st.rainbow_border = None;
            return Err(e);
        }
        let opacity = if st.per_pixel_alpha {
            1.0
        } else {
            st.pal.bg_alpha
        };
        let alpha = (255.0 * opacity * st.fade_factor()).round() as u8;
        if let Some(RenderBackend::Bitmap(_, bitmap)) = &st.target {
            bitmap.present(hwnd, alpha, st.per_pixel_alpha)?;
        } else {
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LWA_ALPHA);
        }
    }
    Ok(())
}

const WM_APP_WARM_UP: u32 = WM_APP + 1;
const WM_APP_LIVE: u32 = WM_APP + 2;

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // A panic must not unwind into the host application (that aborts it).
    std::panic::catch_unwind(|| unsafe { wndproc_inner(hwnd, msg, wparam, lparam) })
        .unwrap_or(LRESULT(0))
}

unsafe fn wndproc_inner(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                BeginPaint(hwnd, &mut ps);
                if GetWindowLongPtrW(hwnd, GWLP_USERDATA) != 0 {
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        if let Some(Err(error)) = with_state(hwnd, |st| render(hwnd, st)) {
                            crate::debug_log!("render candidate window: {error}");
                        }
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
                    let y_dip = y / st.scale;
                    let cur_h = st
                        .last_rect
                        .map_or_else(|| st.animated_size().1, |(_, _, _, h)| h as f32 / st.scale);
                    let rows_top = st.rows_top();
                    let rows_bottom = (cur_h - st.padding() - FOOTER_H).max(rows_top);
                    if !st.visible || y_dip < rows_top || y_dip >= rows_bottom {
                        return None;
                    }
                    let row = ((y_dip - rows_top) / ROW_H).floor();
                    if row >= 0.0 && (row as usize) < st.page_len {
                        st.on_click
                            .clone()
                            .map(|f| (f, st.page_start + row as usize))
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
                    m.iter()
                        .position(|(h, _)| *h == hwnd.0 as isize)
                        .map(|i| m.remove(i))
                };
                let handler = with_state(hwnd, |st| st.on_live.clone()).flatten();
                if let (Some((_, result)), Some(f)) = (item, handler) {
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(result)));
                }
                LRESULT(0)
            }
            WM_APP_WARM_UP => {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    with_state(hwnd, |st| {
                        if st.target.is_none() && !st.bitmap_popup {
                            let _ = render(hwnd, st);
                        }
                    });
                }));
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == RELAYOUT_TIMER_ID => {
                let _ = KillTimer(Some(hwnd), RELAYOUT_TIMER_ID);
                let handler = with_state(hwnd, |st| st.on_relayout.clone()).flatten();
                if let Some(f) = handler {
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f()));
                }
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == HIDE_TIMER_ID => {
                hide_now(hwnd);
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == RAINBOW_TIMER_ID => {
                if with_state(hwnd, |st| st.visible && st.gaming_mode && !st.animating())
                    == Some(true)
                {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == TIMER_ID => {
                let fade_done = with_state(hwnd, |st| {
                    st.fading_out.is_some_and(|(start, _)| {
                        start.elapsed().as_secs_f32() * 1000.0 >= FADE_OUT_MS
                    })
                })
                .unwrap_or(false);
                if fade_done {
                    finish_hide(hwnd);
                    return LRESULT(0);
                }
                let resize_rect = with_state(hwnd, |st| {
                    if !st.visible {
                        return None;
                    }
                    let placement = st.placement?;
                    let (cur_w, cur_h) = st.animated_size();
                    let w_px = (cur_w * st.scale).round().max(1.0) as i32;
                    let h_px = (cur_h * st.scale).round().max(1.0) as i32;
                    let rect = placement.rect(w_px, h_px);
                    if st.last_rect != Some(rect) {
                        st.last_rect = Some(rect);
                        Some(rect)
                    } else {
                        None
                    }
                })
                .flatten();
                if let Some((x, y, w, h)) = resize_rect {
                    let _ = SetWindowPos(
                        hwnd,
                        None,
                        x,
                        y,
                        w,
                        h,
                        SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOREDRAW,
                    );
                    position_backdrop(hwnd, (x, y, w, h));
                }
                let animating = with_state(hwnd, |st| {
                    let target_w = (st.size_to.0 * st.scale).round().max(1.0) as i32;
                    let target_h = (st.size_to.1 * st.scale).round().max(1.0) as i32;
                    let pending_size = st.visible
                        && st
                            .last_rect
                            .is_some_and(|(_, _, w, h)| w != target_w || h != target_h);
                    (st.visible || st.fading_out.is_some()) && (st.animating() || pending_size)
                })
                .unwrap_or(false);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resize_keeps_its_anchor_at_the_screen_edge() {
        let work = RECT { left: 0, top: 0, right: 800, bottom: 600 };
        let anchor = RECT { left: 760, top: 520, right: 780, bottom: 540 };
        let small = choose_placement(anchor, work, 100, 40, 20.0, None);
        assert_eq!(small.rect(100, 40), (700, 544, 100, 40));

        let growing = choose_placement(anchor, work, 250, 160, 20.0, Some(small));
        assert_eq!(growing.rect(100, 40), (550, 476, 100, 40));
        assert_eq!(growing.rect(250, 160), (550, 356, 250, 160));

        let shrinking = choose_placement(anchor, work, 250, 40, 20.0, Some(growing));
        let settled = choose_placement(anchor, work, 100, 40, 20.0, Some(shrinking));
        assert_eq!(settled.rect(100, 40), (550, 476, 100, 40));
    }

    #[test]
    fn bitmap_popup_draws_candidate_text_and_presents_it() {
        use windows::Win32::Graphics::Gdi::{DIBSECTION, GetObjectW, GetPixel};

        let popup = CandidateWindow::new(None, true).unwrap();
        let candidates = CandidateList {
            items: vec!["候補".into()],
            selected: 0,
            page_size: 9,
            kind: ListKind::Suggestion,
        };
        let anchor = RECT {
            left: 80,
            top: 80,
            right: 120,
            bottom: 100,
        };
        popup.show(Some(&candidates), None, anchor, false, false, "");
        with_state(popup.hwnd, |state| {
            render(popup.hwnd, state).unwrap();
            let Some(RenderBackend::Bitmap(_, bitmap)) = &state.target else {
                panic!("expected a bitmap render target");
            };
            let background = unsafe { GetPixel(bitmap.dc, 0, 0) };
            assert_ne!(background, COLORREF(u32::MAX));
            let top = (state.padding() * state.scale) as i32;
            let bottom = ((state.padding() + ROW_H) * state.scale) as i32;
            assert!((top..bottom).any(|y| {
                (0..bitmap.size.width as i32).any(|x| {
                    let color = unsafe { GetPixel(bitmap.dc, x, y) };
                    color != background && color != COLORREF(u32::MAX)
                })
            }));

            state.pal.bg_alpha = 0.55;
            state.per_pixel_alpha = true;
            state.target = None;
            render(popup.hwnd, state).unwrap();
            let Some(RenderBackend::Bitmap(_, bitmap)) = &state.target else {
                panic!("expected a premultiplied bitmap render target");
            };
            let mut section = DIBSECTION::default();
            unsafe {
                let size = std::mem::size_of::<DIBSECTION>();
                assert_eq!(
                    GetObjectW(
                        bitmap.bitmap.into(),
                        size as i32,
                        Some((&raw mut section).cast())
                    ),
                    size as i32
                );
                let pixels = std::slice::from_raw_parts(
                    section.dsBm.bmBits.cast::<u8>(),
                    section.dsBm.bmWidthBytes as usize * bitmap.size.height as usize,
                );
                assert_eq!(pixels[3], 140);
                assert!(pixels.chunks_exact(4).any(|pixel| pixel[3] == 255));
            }
        })
        .unwrap();
    }

    #[test]
    fn uses_installed_candidate_font_and_falls_back_when_missing() {
        unsafe {
            let factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).unwrap();
            let family = |name| {
                let format = candidate_format(&factory, name).unwrap();
                let len = format.GetFontFamilyNameLength() as usize;
                let mut output = vec![0; len + 1];
                format.GetFontFamilyName(&mut output).unwrap();
                String::from_utf16(&output[..len]).unwrap()
            };
            assert_eq!(family("Arial"), "Arial");
            assert_eq!(family("Sweet IME nonexistent test font"), family(""));
        }
    }

    #[test]
    fn resize_animation_interpolates_width_and_height_smoothly() {
        let start = (140.0, 80.0);
        let end = (260.0, 200.0);
        assert_eq!(interpolate_size(start, end, 0.0), start);
        let mid = interpolate_size(start, end, RESIZE_MS * 0.5);
        // ease-out cubic at t = 0.5 is 1 - 0.5^3 = 0.875.
        assert!((mid.0 - (140.0 + 120.0 * 0.875)).abs() < 1e-3);
        assert!((mid.1 - (80.0 + 120.0 * 0.875)).abs() < 1e-3);
        assert_eq!(interpolate_size(start, end, RESIZE_MS), end);
        assert_eq!(interpolate_size(start, end, RESIZE_MS * 2.0), end);

        let popup = CandidateWindow::new(None, true).unwrap();
        let anchor = RECT {
            left: 80,
            top: 80,
            right: 120,
            bottom: 100,
        };
        let short_list = CandidateList {
            items: vec!["あ".into()],
            selected: 0,
            page_size: 9,
            kind: ListKind::Conversion,
        };
        popup.show(Some(&short_list), None, anchor, false, false, "");
        // First show starts directly at target size without animating from zero.
        let initial_to = with_state(popup.hwnd, |st| {
            assert!(!st.is_resizing());
            assert_eq!(st.size_from, st.size_to);
            st.size_to
        })
        .unwrap();

        let tall_list = CandidateList {
            items: vec![
                "とても長い変換候補のテキスト".into(),
                "二つ目の候補".into(),
                "三つ目の候補".into(),
            ],
            selected: 0,
            page_size: 9,
            kind: ListKind::Conversion,
        };
        popup.show(Some(&tall_list), None, anchor, false, false, "");
        with_state(popup.hwnd, |st| {
            assert_eq!(st.size_from, initial_to);
            assert!(st.size_to.0 > initial_to.0);
            assert!(st.size_to.1 > initial_to.1);
            assert!(st.is_resizing());
        })
        .unwrap();
    }
}
