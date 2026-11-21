//! Input mode indicator: a language bar button registered as
//! `GUID_LBI_INPUTMODE`, which Windows shows in the taskbar ("あ" / "A").
//! Clicking it toggles the IME on and off.

use std::cell::{Cell, RefCell};

use windows::Win32::Foundation::{E_INVALIDARG, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    ANTIALIASED_QUALITY, BI_RGB, BITMAPINFO, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, BITMAPINFOHEADER, CreateBitmap, CreateCompatibleDC, CreateDIBSection,
    CreateFontW, DIB_RGB_COLORS, DT_CENTER, DT_SINGLELINE, DT_VCENTER, DeleteDC, DeleteObject,
    DrawTextW, FW_BOLD, HGDIOBJ, SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
};
use windows::Win32::UI::TextServices::{
    GUID_LBI_INPUTMODE, ITfLangBarItem, ITfLangBarItem_Impl, ITfLangBarItemButton,
    ITfLangBarItemButton_Impl, ITfLangBarItemSink, ITfMenu, ITfSource, ITfSource_Impl,
    TF_LANGBARITEMINFO, TF_LBI_ICON, TF_LBI_STYLE_BTN_BUTTON, TF_LBI_STYLE_SHOWNINTRAY,
    TF_LBI_CLK_RIGHT, TF_LBI_TEXT, TF_LBI_TOOLTIP, TfLBIClick,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateIconIndirect, GetSystemMetrics, HICON, ICONINFO, SM_CXSMICON,
};
use windows_core::{BSTR, GUID, IUnknown, Interface, Ref, Result, implement, w};

use crate::CLSID_SWEET_IME;

const SINK_COOKIE: u32 = 0x5357_4545;
// olectl.h
const CONNECT_E_NOCONNECTION: windows_core::HRESULT = windows_core::HRESULT(0x8004_0200_u32 as i32);
const CONNECT_E_ADVISELIMIT: windows_core::HRESULT = windows_core::HRESULT(0x8004_0201_u32 as i32);
const CONNECT_E_CANNOTCONNECT: windows_core::HRESULT = windows_core::HRESULT(0x8004_0202_u32 as i32);

#[implement(ITfLangBarItemButton, ITfLangBarItem, ITfSource)]
pub struct ModeButton {
    open: Cell<bool>,
    sink: RefCell<Option<ITfLangBarItemSink>>,
    on_click: RefCell<Option<Box<dyn Fn()>>>,
}

impl ModeButton {
    pub fn new(open: bool, on_click: impl Fn() + 'static) -> Self {
        Self { open: Cell::new(open), sink: RefCell::new(None), on_click: RefCell::new(Some(Box::new(on_click))) }
    }

    /// Updates the shown mode and notifies the language bar.
    pub fn set_open(&self, open: bool) {
        if self.open.replace(open) == open {
            return;
        }
        let sink = self.sink.borrow().clone();
        if let Some(sink) = sink {
            unsafe {
                let _ = sink.OnUpdate(TF_LBI_ICON | TF_LBI_TEXT | TF_LBI_TOOLTIP);
            }
        }
    }

    /// Drops the click handler (it references the text service).
    pub fn detach(&self) {
        self.on_click.borrow_mut().take();
        self.sink.borrow_mut().take();
    }

    fn label(&self) -> &'static str {
        if self.open.get() { "あ" } else { "A" }
    }
}

impl ITfLangBarItem_Impl for ModeButton_Impl {
    fn GetInfo(&self, pinfo: *mut TF_LANGBARITEMINFO) -> Result<()> {
        if pinfo.is_null() {
            return Err(E_INVALIDARG.into());
        }
        let mut desc = [0u16; 32];
        for (d, s) in desc.iter_mut().zip("Sweet IME input mode".encode_utf16()) {
            *d = s;
        }
        unsafe {
            *pinfo = TF_LANGBARITEMINFO {
                clsidService: CLSID_SWEET_IME,
                guidItem: GUID_LBI_INPUTMODE,
                dwStyle: TF_LBI_STYLE_BTN_BUTTON | TF_LBI_STYLE_SHOWNINTRAY,
                ulSort: 0,
                szDescription: desc,
            };
        }
        Ok(())
    }

    fn GetStatus(&self) -> Result<u32> {
        Ok(0)
    }

    fn Show(&self, _fshow: windows_core::BOOL) -> Result<()> {
        Ok(())
    }

    fn GetTooltipString(&self) -> Result<BSTR> {
        Ok(BSTR::from(if self.open.get() {
            "ひらがな (Sweet IME)・右クリックで設定"
        } else {
            "半角英数 (Sweet IME)・右クリックで設定"
        }))
    }
}

impl ITfLangBarItemButton_Impl for ModeButton_Impl {
    fn OnClick(&self, click: TfLBIClick, _pt: &POINT, _area: *const RECT) -> Result<()> {
        crate::guard(|| {
            if click == TF_LBI_CLK_RIGHT {
                crate::launch_settings();
                return Ok(());
            }
            // Take the handler out while calling it: it re-enters set_open.
            let f = self.on_click.borrow_mut().take();
            if let Some(f) = f {
                f();
                let mut slot = self.on_click.borrow_mut();
                if slot.is_none() {
                    *slot = Some(f);
                }
            }
            Ok(())
        })
    }

    fn InitMenu(&self, _pmenu: Ref<ITfMenu>) -> Result<()> {
        Ok(())
    }

    fn OnMenuSelect(&self, _wid: u32) -> Result<()> {
        Ok(())
    }

    fn GetIcon(&self) -> Result<HICON> {
        // The caller owns (and destroys) the returned icon.
        make_icon(self.label())
    }

    fn GetText(&self) -> Result<BSTR> {
        Ok(BSTR::from(self.label()))
    }
}

impl ITfSource_Impl for ModeButton_Impl {
    fn AdviseSink(&self, riid: *const GUID, punk: Ref<IUnknown>) -> Result<u32> {
        if riid.is_null() || unsafe { *riid } != ITfLangBarItemSink::IID {
            return Err(CONNECT_E_CANNOTCONNECT.into());
        }
        if self.sink.borrow().is_some() {
            return Err(CONNECT_E_ADVISELIMIT.into());
        }
        let sink: ITfLangBarItemSink = punk.ok()?.cast()?;
        *self.sink.borrow_mut() = Some(sink);
        Ok(SINK_COOKIE)
    }

    fn UnadviseSink(&self, cookie: u32) -> Result<()> {
        if cookie != SINK_COOKIE || self.sink.borrow_mut().take().is_none() {
            return Err(CONNECT_E_NOCONNECTION.into());
        }
        Ok(())
    }
}

/// Whether the taskbar uses the light theme (dark glyph needed).
fn light_taskbar() -> bool {
    crate::candidate_window::read_dword(
        w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
        w!("SystemUsesLightTheme"),
    )
    .is_some_and(|v| v != 0)
}

/// Renders `text` into a small-icon-sized HICON with an alpha channel.
fn make_icon(text: &str) -> Result<HICON> {
    let size = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16);
    let fg: u32 = if light_taskbar() { 0x00_00_00 } else { 0xFF_FF_FF };
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
        let color = CreateDIBSection(Some(dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0)?;
        let old_bmp = SelectObject(dc, HGDIOBJ(color.0));
        // GDI text has no alpha: draw white on black and use the coverage
        // as alpha.
        let font = CreateFontW(
            -(size * 15 / 16), 0, 0, 0, FW_BOLD.0 as i32, 0, 0, 0, DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, ANTIALIASED_QUALITY, 0,
            w!("Yu Gothic UI"),
        );
        let old_font = SelectObject(dc, HGDIOBJ(font.0));
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, windows::Win32::Foundation::COLORREF(0xFF_FF_FF));
        let mut rc = RECT { left: 0, top: 0, right: size, bottom: size };
        let mut wtext: Vec<u16> = text.encode_utf16().collect();
        DrawTextW(dc, &mut wtext, &mut rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        let px = std::slice::from_raw_parts_mut(bits as *mut u32, (size * size) as usize);
        for p in px.iter_mut() {
            let r = (*p >> 16) & 0xFF;
            let g = (*p >> 8) & 0xFF;
            let b = *p & 0xFF;
            let a = r.max(g).max(b);
            *p = (a << 24) | fg;
        }
        SelectObject(dc, old_font);
        SelectObject(dc, old_bmp);
        let _ = DeleteObject(HGDIOBJ(font.0));
        let _ = DeleteDC(dc);
        let mask = CreateBitmap(size, size, 1, 1, None);
        let info = ICONINFO { fIcon: true.into(), xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
        let icon = CreateIconIndirect(&info);
        let _ = DeleteObject(HGDIOBJ(mask.0));
        let _ = DeleteObject(HGDIOBJ(color.0));
        icon
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;
    use windows::Win32::UI::TextServices::{ITfLangBarItemSink_Impl, TF_LBI_CLK_LEFT};
    use windows_core::ComObject;

    #[implement(ITfLangBarItemSink)]
    struct Sink(Rc<Cell<u32>>);

    impl ITfLangBarItemSink_Impl for Sink_Impl {
        fn OnUpdate(&self, flags: u32) -> Result<()> {
            self.0.set(self.0.get() | flags);
            Ok(())
        }
    }

    #[test]
    fn button_reports_mode_and_toggles() {
        let clicks = Rc::new(Cell::new(0));
        let c2 = clicks.clone();
        let obj = ComObject::new(ModeButton::new(true, move || c2.set(c2.get() + 1)));
        let button: ITfLangBarItemButton = obj.to_interface();
        unsafe {
            let mut info = TF_LANGBARITEMINFO::default();
            button.GetInfo(&mut info).unwrap();
            assert_eq!(info.guidItem, GUID_LBI_INPUTMODE);
            assert_eq!(info.clsidService, CLSID_SWEET_IME);
            assert_eq!(button.GetText().unwrap().to_string(), "あ");

            let updates = Rc::new(Cell::new(0));
            let sink: ITfLangBarItemSink = Sink(updates.clone()).into();
            let src: ITfSource = button.cast().unwrap();
            let cookie = src.AdviseSink(&ITfLangBarItemSink::IID, &sink).unwrap();
            obj.set_open(false);
            assert_eq!(button.GetText().unwrap().to_string(), "A");
            assert_ne!(updates.get() & TF_LBI_ICON, 0);

            button.OnClick(TF_LBI_CLK_LEFT, POINT::default(), &RECT::default()).unwrap();
            button.OnClick(TF_LBI_CLK_LEFT, POINT::default(), &RECT::default()).unwrap();
            assert_eq!(clicks.get(), 2);
            src.UnadviseSink(cookie).unwrap();
            obj.detach();
            button.OnClick(TF_LBI_CLK_LEFT, POINT::default(), &RECT::default()).unwrap();
            assert_eq!(clicks.get(), 2);
        }
    }

    #[test]
    fn icon_renders_glyph() {
        for t in ["あ", "A"] {
            let icon = super::make_icon(t).expect("icon");
            assert!(!icon.is_invalid());
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::DestroyIcon(icon);
            }
        }
    }
}
