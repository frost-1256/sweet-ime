//! Small "あ" / "A" badge shown near the caret when the input mode changes.
//!
//! A click-through layered popup: the bitmap is drawn once per show with
//! GDI (text coverage) plus an anti-aliased rounded rectangle, then faded
//! out with the layered window's constant alpha.

use std::cell::RefCell;
use std::sync::Once;
use std::time::Instant;

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    ANTIALIASED_QUALITY, AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
    CLIP_DEFAULT_PRECIS, CreateCompatibleDC, CreateDIBSection, CreateFontW, DEFAULT_CHARSET, DIB_RGB_COLORS,
    DT_CENTER, DT_SINGLELINE, DT_VCENTER, DeleteDC, DeleteObject, DrawTextW, FW_SEMIBOLD, GetMonitorInfoW,
    HGDIOBJ, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint, OUT_DEFAULT_PRECIS, SelectObject,
    SetBkMode, SetTextColor, TRANSPARENT,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_HWNDPARENT, HWND_TOPMOST, KillTimer, MA_NOACTIVATE,
    RegisterClassExW, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SetTimer, SetWindowLongPtrW, SetWindowPos,
    ShowWindow, ULW_ALPHA, UpdateLayeredWindow, WM_MOUSEACTIVATE, WM_TIMER, WNDCLASSEXW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows_core::{PCWSTR, Result, w};

const CLASS_NAME: PCWSTR = w!("SweetImeModeIndicator");
const TIMER_HOLD: usize = 1;
const TIMER_FADE: usize = 2;
const HOLD_MS: u32 = 900;
const FADE_MS: f32 = 250.0;
// Layout in DIPs.
const SIZE_DIP: f32 = 28.0;
const RADIUS_DIP: f32 = 6.0;
const GAP_DIP: f32 = 4.0;

thread_local! {
    /// Fade start per indicator window.
    static FADES: RefCell<Vec<(isize, Instant)>> = const { RefCell::new(Vec::new()) };
}

pub struct ModeIndicator {
    hwnd: HWND,
}

fn register_class() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: crate::dll_instance(),
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        RegisterClassExW(&wc);
    });
}

fn light_theme() -> bool {
    crate::candidate_window::read_dword(
        w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
        w!("AppsUseLightTheme"),
    )
    .unwrap_or(1)
        != 0
}

fn blend(alpha: u8) -> BLENDFUNCTION {
    BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: alpha,
        AlphaFormat: AC_SRC_ALPHA as u8,
    }
}

impl ModeIndicator {
    pub fn new() -> Result<Self> {
        register_class();
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED | WS_EX_TRANSPARENT,
                CLASS_NAME,
                PCWSTR::null(),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(crate::dll_instance()),
                None,
            )?
        };
        Ok(Self { hwnd })
    }

    /// Shows the badge below `caret` (screen coordinates) for a moment.
    pub fn show(&self, open: bool, caret: RECT, owner: Option<HWND>) {
        let hwnd = self.hwnd;
        unsafe {
            if let Some(o) = owner {
                SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, o.0 as _);
            }
            // Move first so the DPI is the one of the caret's monitor.
            let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), caret.left, caret.bottom, 1, 1, SWP_NOACTIVATE);
            let dpi = GetDpiForWindow(hwnd);
            let scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
            let size = (SIZE_DIP * scale).round() as i32;
            let gap = (GAP_DIP * scale).round() as i32;
            let pt = POINT { x: caret.left, y: caret.bottom };
            let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
            let work = if GetMonitorInfoW(MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST), &mut mi).as_bool() {
                mi.rcWork
            } else {
                RECT { left: i32::MIN / 2, top: i32::MIN / 2, right: i32::MAX / 2, bottom: i32::MAX / 2 }
            };
            let x = caret.left.min(work.right - size).max(work.left);
            let mut y = caret.bottom + gap;
            if y + size > work.bottom {
                y = caret.top - gap - size;
            }
            if draw(hwnd, POINT { x, y }, size, scale, if open { "あ" } else { "A" }).is_err() {
                return;
            }
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            let _ = KillTimer(Some(hwnd), TIMER_FADE);
            SetTimer(Some(hwnd), TIMER_HOLD, HOLD_MS, None);
        }
    }

    pub fn hide(&self) {
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_HOLD);
            let _ = KillTimer(Some(self.hwnd), TIMER_FADE);
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }
}

impl Drop for ModeIndicator {
    fn drop(&mut self) {
        FADES.with(|f| f.borrow_mut().retain(|(h, _)| *h != self.hwnd.0 as isize));
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

/// Coverage of a rounded rectangle (`size` square, radius `r`, inset by
/// `inset`) at pixel (x, y), anti-aliased over one pixel.
fn rounded_coverage(x: i32, y: i32, size: f32, r: f32, inset: f32) -> f32 {
    let h = size / 2.0 - inset;
    let r = (r - inset).max(0.0);
    let px = (x as f32 + 0.5 - size / 2.0).abs() - (h - r);
    let py = (y as f32 + 0.5 - size / 2.0).abs() - (h - r);
    let outside = (px.max(0.0).powi(2) + py.max(0.0).powi(2)).sqrt();
    let d = outside + px.max(py).min(0.0) - r;
    (0.5 - d).clamp(0.0, 1.0)
}

/// Renders the badge into a premultiplied 32-bit bitmap and puts it on the
/// layered window at `pos`.
fn draw(hwnd: HWND, pos: POINT, size: i32, scale: f32, text: &str) -> Result<()> {
    let light = light_theme();
    let (bg, border, fg): ([f32; 3], [f32; 3], [f32; 3]) = if light {
        ([249.0, 249.0, 249.0], [200.0, 200.0, 200.0], [26.0, 26.0, 26.0])
    } else {
        ([44.0, 44.0, 44.0], [90.0, 90.0, 90.0], [255.0, 255.0, 255.0])
    };
    unsafe {
        let dc = CreateCompatibleDC(None);
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size,
                biHeight: -size, // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
        let bmp = match CreateDIBSection(Some(dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
            Ok(b) => b,
            Err(e) => {
                let _ = DeleteDC(dc);
                return Err(e);
            }
        };
        let old_bmp = SelectObject(dc, HGDIOBJ(bmp.0));
        // Text coverage: white on black (GDI has no alpha).
        let font = CreateFontW(
            -((size as f32 * 0.6).round() as i32), 0, 0, 0, FW_SEMIBOLD.0 as i32, 0, 0, 0, DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, ANTIALIASED_QUALITY, 0, w!("Yu Gothic UI"),
        );
        let old_font = SelectObject(dc, HGDIOBJ(font.0));
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, COLORREF(0xFF_FF_FF));
        let mut rc = RECT { left: 0, top: 0, right: size, bottom: size };
        let mut wtext: Vec<u16> = text.encode_utf16().collect();
        DrawTextW(dc, &mut wtext, &mut rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        let px = std::slice::from_raw_parts_mut(bits as *mut u32, (size * size) as usize);
        let border_w = scale.max(1.0);
        let radius = RADIUS_DIP * scale;
        for y in 0..size {
            for x in 0..size {
                let i = (y * size + x) as usize;
                let p = px[i];
                let cov = (((p >> 16) & 0xFF).max((p >> 8) & 0xFF).max(p & 0xFF)) as f32 / 255.0;
                let shape = rounded_coverage(x, y, size as f32, radius, 0.0);
                if shape <= 0.0 {
                    px[i] = 0;
                    continue;
                }
                let inner = rounded_coverage(x, y, size as f32, radius, border_w);
                let mut c = [0.0f32; 3];
                for k in 0..3 {
                    let base = (bg[k] * inner + border[k] * (shape - inner)) / shape;
                    c[k] = (base * (1.0 - cov) + fg[k] * cov) * shape;
                }
                let a = (shape * 255.0).round() as u32;
                px[i] = (a << 24) | ((c[0].round() as u32) << 16) | ((c[1].round() as u32) << 8) | c[2].round() as u32;
            }
        }
        let r = UpdateLayeredWindow(
            hwnd,
            None,
            Some(&pos),
            Some(&SIZE { cx: size, cy: size }),
            Some(dc),
            Some(&POINT { x: 0, y: 0 }),
            COLORREF(0),
            Some(&blend(255)),
            ULW_ALPHA,
        );
        SelectObject(dc, old_font);
        SelectObject(dc, old_bmp);
        let _ = DeleteObject(HGDIOBJ(font.0));
        let _ = DeleteObject(HGDIOBJ(bmp.0));
        let _ = DeleteDC(dc);
        r
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            WM_TIMER if wparam.0 == TIMER_HOLD => {
                let _ = KillTimer(Some(hwnd), TIMER_HOLD);
                FADES.with(|f| {
                    let mut f = f.borrow_mut();
                    f.retain(|(h, _)| *h != hwnd.0 as isize);
                    f.push((hwnd.0 as isize, Instant::now()));
                });
                SetTimer(Some(hwnd), TIMER_FADE, 15, None);
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == TIMER_FADE => {
                let start = FADES.with(|f| f.borrow().iter().find(|(h, _)| *h == hwnd.0 as isize).map(|x| x.1));
                let t = start.map_or(1.0, |s| s.elapsed().as_secs_f32() * 1000.0 / FADE_MS);
                if t >= 1.0 {
                    let _ = KillTimer(Some(hwnd), TIMER_FADE);
                    let _ = ShowWindow(hwnd, SW_HIDE);
                } else {
                    let a = ((1.0 - t) * 255.0) as u8;
                    let _ = UpdateLayeredWindow(hwnd, None, None, None, None, None, COLORREF(0), Some(&blend(a)), ULW_ALPHA);
                }
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}
