//! Uses the *installed* Sweet IME the way an application does: activates
//! its profile for this process, focuses a real text control and sends
//! keystrokes with SendInput. Reports whether the mode badge appeared.
//!
//!   cargo run --release -p sweet-tsf --example sysctl -- [edit|richedit] [dev sweet_tsf.dll]
//!
//! Brings its window to the foreground for a few seconds.

use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx};
use windows::Win32::System::LibraryLoader::LoadLibraryW;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::TextServices::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows_core::{GUID, Interface, PCWSTR, Result, w};

const CLSID_SWEET_IME: GUID = GUID::from_u128(0x7e1d3c2a_5b8f_4c1e_9a3d_2f6b8c4e1a01);
const GUID_PROFILE: GUID = GUID::from_u128(0x7e1d3c2a_5b8f_4c1e_9a3d_2f6b8c4e1a02);

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

fn key(vk: u16, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: 0,
                dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Presses keys by scan code, so the keyboard layout translates them
/// exactly as for a physical key press. Extended scan codes: 0xE0xx.
fn press_scan(chord: &[u16]) {
    let mk = |sc: u16, up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: sc & 0xFF,
                dwFlags: KEYEVENTF_SCANCODE
                    | if sc & 0xE000 == 0xE000 { KEYEVENTF_EXTENDEDKEY } else { KEYBD_EVENT_FLAGS(0) }
                    | if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let mut v: Vec<INPUT> = chord.iter().map(|&k| mk(k, false)).collect();
    v.extend(chord.iter().rev().map(|&k| mk(k, true)));
    unsafe {
        SendInput(&v, std::mem::size_of::<INPUT>() as i32);
    }
}

fn press(chord: &[u16]) {
    let mut v: Vec<INPUT> = chord.iter().map(|&k| key(k, false)).collect();
    v.extend(chord.iter().rev().map(|&k| key(k, true)));
    unsafe {
        SendInput(&v, std::mem::size_of::<INPUT>() as i32);
    }
}

/// Delivers a chord through the thread's keystroke manager (no focus
/// stealing). Modifiers are set in the thread keyboard state.
fn send(tm: &ITfThreadMgr, chord: &[u16]) -> bool {
    unsafe {
        let km: ITfKeystrokeMgr = tm.cast().unwrap();
        let mut ks = [0u8; 256];
        let _ = GetKeyboardState(&mut ks);
        let (mods, k) = chord.split_at(chord.len() - 1);
        for &m in mods {
            ks[m as usize] = 0x80;
        }
        let _ = SetKeyboardState(&ks);
        let vk = k[0];
        let scan = MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC);
        let lp = LPARAM(((scan as isize) << 16) | 1);
        let wp = WPARAM(vk as usize);
        let eaten = km.TestKeyDown(wp, lp).is_ok_and(|b| b.as_bool()) && km.KeyDown(wp, lp).is_ok_and(|b| b.as_bool());
        let _ = km.KeyUp(wp, LPARAM(lp.0 | (3 << 30)));
        for &m in mods {
            ks[m as usize] = 0;
        }
        let _ = SetKeyboardState(&ks);
        eaten
    }
}

fn open_state(tm: &ITfThreadMgr) -> Option<i32> {
    unsafe {
        let c = tm.cast::<ITfCompartmentMgr>().ok()?.GetCompartment(&GUID_COMPARTMENT_KEYBOARD_OPENCLOSE).ok()?;
        i32::try_from(&c.GetValue().ok()?).ok()
    }
}

fn main() -> Result<()> {
    let kind = std::env::args().nth(1).unwrap_or_else(|| "edit".into());
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        // Optionally serve the TIP from a development DLL, for this process
        // only, instead of the installed one.
        if let Some(dll) = std::env::args().nth(2) {
            use windows::Win32::System::Com::{CoRegisterClassObject, IClassFactory, REGCLS_MULTIPLEUSE};
            let module = LoadLibraryW(&windows_core::HSTRING::from(dll.as_str()))?;
            let proc = windows::Win32::System::LibraryLoader::GetProcAddress(module, windows_core::s!("DllGetClassObject"))
                .expect("DllGetClassObject");
            type GetClassObject = unsafe extern "system" fn(*const GUID, *const GUID, *mut *mut std::ffi::c_void) -> windows_core::HRESULT;
            let get: GetClassObject = std::mem::transmute(proc);
            let mut f: *mut std::ffi::c_void = std::ptr::null_mut();
            get(&CLSID_SWEET_IME, &IClassFactory::IID, &mut f).ok()?;
            let factory = IClassFactory::from_raw(f);
            let cookie = CoRegisterClassObject(&CLSID_SWEET_IME, &factory, CLSCTX_INPROC_SERVER, REGCLS_MULTIPLEUSE)?;
            println!("serving TIP from {dll} (cookie {cookie})");
            std::mem::forget(factory);
        }
        let top = CreateWindowExW(
            Default::default(), w!("STATIC"), w!("sweet sysctl"), WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            100, 100, 420, 160, None, None, None, None,
        )?;
        let class: PCWSTR = if kind == "richedit" {
            let _ = LoadLibraryW(w!("msftedit.dll"));
            w!("RICHEDIT50W")
        } else {
            w!("EDIT")
        };
        let ctl = CreateWindowExW(
            Default::default(), class, w!(""), WS_CHILD | WS_VISIBLE | WS_BORDER,
            10, 10, 380, 60, Some(top), None, None, None,
        )?;
        if kind == "richedit" {
            SendMessageW(ctl, 0x04CC, Some(WPARAM(0x10000)), Some(LPARAM(0x10000)));
        }
        let profiles: ITfInputProcessorProfileMgr =
            CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER)?;
        profiles.ActivateProfile(
            TF_PROFILETYPE_INPUTPROCESSOR,
            0x0411,
            &CLSID_SWEET_IME,
            &GUID_PROFILE,
            HKL::default(),
            TF_IPPMF_FORPROCESS | TF_IPPMF_DONTCARECURRENTINPUTLANGUAGE,
        )?;
        // Take the foreground (a background process may not, unless it
        // shares input with the current foreground thread).
        {
            use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
            let fg = GetWindowThreadProcessId(GetForegroundWindow(), None);
            let me = GetCurrentThreadId();
            let _ = AttachThreadInput(fg, me, true);
            let _ = SetForegroundWindow(top);
            let _ = AttachThreadInput(fg, me, false);
        }
        let _ = SetFocus(Some(ctl));
        pump_for(800);
        let tm: ITfThreadMgr = CoCreateInstance(&CLSID_TF_ThreadMgr, None, CLSCTX_INPROC_SERVER)?;
        println!("{kind}: foreground ok={} open={:?}", GetForegroundWindow() == top, open_state(&tm));
        println!("after activation: badge={:?} focus doc={}", badge(), tm.GetFocus().is_ok());
        let mut prof = TF_INPUTPROCESSORPROFILE::default();
        let _ = profiles.GetActiveProfile(&GUID_TFCAT_TIP_KEYBOARD, &mut prof);
        println!("active profile is Sweet IME: {}", prof.clsid == CLSID_SWEET_IME);
        let tests: [(&str, &[u16]); 4] = [
            ("VK_KANJI", &[0x19]),
            ("VK_OEM_AUTO (半角/全角)", &[0xF3]),
            ("Alt+`", &[0x12, 0xC0]),
            ("無変換", &[0x1D]),
        ];
        for (name, chord) in tests {
            pump_for(1500); // let the previous badge go
            let eaten = if std::env::var_os("SWEET_USE_KM").is_some() { send(&tm, chord) } else { press(chord); true };
            pump_for(150);
            println!("{name}: eaten={eaten} open={:?} badge={:?}", open_state(&tm), badge());
        }
        // Physical-key presses through the layout (scan codes).
        for (name, chord) in [("scan Alt+` (x2)", &[0x38u16, 0x29][..]), ("scan Ctrl+Space", &[0x1D, 0x39][..])] {
            pump_for(1500);
            let before = open_state(&tm);
            press_scan(chord);
            pump_for(150);
            println!("{name}: open {before:?} -> {:?} badge={:?}", open_state(&tm), badge());
        }
        // Type something to see the IME is really active here.
        press(&[0x1C]); // 変換 = on
        pump_for(100);
        for k in [0x41u16, 0x49] {
            press(&[k]);
        }
        pump_for(200);
        let mut buf = [0u16; 64];
        let n = GetWindowTextW(ctl, &mut buf);
        println!("typed: {:?}", String::from_utf16_lossy(&buf[..n as usize]));
        press(&[0x1B]);
        press(&[0x1B]);
        pump_for(100);
        let _ = DestroyWindow(top);
    }
    Ok(())
}
