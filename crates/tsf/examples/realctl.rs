//! Hosts the TIP on a real Win32 text control (EDIT or RichEdit) to check
//! behaviour that depends on the application's TSF document, such as the
//! caret position used for the mode badge.
//!
//!   set SWEET_IME_DEBUG=1
//!   cargo run --release -p sweet-tsf --example realctl -- [edit|richedit] [path\to\sweet_tsf.dll]

use std::ffi::c_void;

use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, IClassFactory};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::TextServices::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows_core::{GUID, HRESULT, HSTRING, Interface, PCWSTR, Result, s, w};

const CLSID_SWEET_IME: GUID = GUID::from_u128(0x7e1d3c2a_5b8f_4c1e_9a3d_2f6b8c4e1a01);
type GetClassObject = unsafe extern "system" fn(*const GUID, *const GUID, *mut *mut c_void) -> HRESULT;

fn pump_for(ms: u64) {
    let t = std::time::Instant::now();
    unsafe {
        loop {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            if t.elapsed().as_millis() as u64 >= ms {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
}

fn badge() -> Option<(bool, RECT)> {
    let pid = std::process::id();
    let mut after: Option<HWND> = None;
    unsafe {
        loop {
            let h = FindWindowExW(None, after, w!("SweetImeModeIndicator"), None).ok()?;
            let mut p = 0;
            GetWindowThreadProcessId(h, Some(&mut p));
            if p == pid {
                let mut rc = RECT::default();
                let _ = GetWindowRect(h, &mut rc);
                return Some((IsWindowVisible(h).as_bool(), rc));
            }
            after = Some(h);
        }
    }
}

fn main() -> Result<()> {
    let kind = std::env::args().nth(1).unwrap_or_else(|| "edit".into());
    let dll = std::env::args().nth(2).unwrap_or_else(|| {
        concat!(env!("CARGO_MANIFEST_DIR"), "\\..\\..\\target\\release\\sweet_tsf.dll").to_string()
    });
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        let tm: ITfThreadMgr = CoCreateInstance(&CLSID_TF_ThreadMgr, None, CLSCTX_INPROC_SERVER)?;
        let tid = tm.Activate()?;

        let top = CreateWindowExW(
            Default::default(), w!("STATIC"), w!("sweet realctl"), WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            100, 100, 420, 160, None, None, None, None,
        )?;
        let class: PCWSTR = if kind == "richedit" {
            let _ = LoadLibraryW(w!("msftedit.dll"));
            w!("RICHEDIT50W")
        } else {
            w!("EDIT")
        };
        let ctl = CreateWindowExW(
            Default::default(), class, w!("abc"), WS_CHILD | WS_VISIBLE | WS_BORDER,
            10, 10, 380, 60, Some(top), None, None, None,
        )?;
        if kind == "richedit" {
            // EM_SETEDITSTYLE(SES_USECTF): use TSF.
            SendMessageW(ctl, 0x04CC, Some(WPARAM(0x10000)), Some(LPARAM(0x10000)));
        }
        let _ = SetForegroundWindow(top);
        let _ = SetFocus(Some(ctl));
        SendMessageW(ctl, 0x00B1 /* EM_SETSEL */, Some(WPARAM(3)), Some(LPARAM(3)));
        pump_for(300);

        let module = LoadLibraryW(&HSTRING::from(dll.as_str()))?;
        let proc = GetProcAddress(module, s!("DllGetClassObject")).expect("DllGetClassObject");
        let get: GetClassObject = std::mem::transmute(proc);
        let mut factory: *mut c_void = std::ptr::null_mut();
        get(&CLSID_SWEET_IME, &IClassFactory::IID, &mut factory).ok()?;
        let factory = IClassFactory::from_raw(factory);
        let tip: ITfTextInputProcessor = factory.CreateInstance(None)?;
        let client = tm.cast::<ITfClientId>()?.GetClientId(&CLSID_SWEET_IME).unwrap_or(tid);
        tip.Activate(&tm, client)?;
        pump_for(200);

        let focus = tm.GetFocus().ok();
        let ctx = focus.as_ref().and_then(|d| d.GetTop().ok());
        println!("{kind}: focus document={} context={}", focus.is_some(), ctx.is_some());
        let mut caret = windows::Win32::Foundation::POINT::default();
        let _ = GetCaretPos(&mut caret);
        let _ = windows::Win32::Graphics::Gdi::ClientToScreen(ctl, &mut caret);
        println!("win32 caret (screen): {},{}", caret.x, caret.y);
        println!("badge after activation: {:?}", badge());

        let sink: ITfKeyEventSink = tip.cast()?;
        if let Some(ctx) = &ctx {
            let lp = LPARAM(1);
            for vk in [0xF3usize, 0xF4] {
                let wp = WPARAM(vk);
                let eaten = sink.OnTestKeyDown(ctx, wp, lp)?.as_bool() && sink.OnKeyDown(ctx, wp, lp)?.as_bool();
                pump_for(100);
                println!("toggle 0x{vk:02X} eaten={eaten} badge={:?}", badge());
            }
        }
        pump_for(1500);
        println!("badge later: {:?}", badge());
        tip.Deactivate()?;
        tm.Deactivate()?;
        let _ = DestroyWindow(top);
    }
    Ok(())
}
