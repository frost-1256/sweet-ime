//! End-to-end TSF harness: loads sweet_tsf_core.dll without registration, hosts it
//! on a real ITfThreadMgr with an in-memory ITextStoreACP, sends keystrokes
//! through ITfKeystrokeMgr and checks the resulting document text.
//!
//!   cargo run --release -p sweet-tsf --example harness -- [path\to\sweet_tsf_core.dll]

use std::cell::{Cell, RefCell};
use std::ffi::c_void;

use windows::Win32::Foundation::{E_INVALIDARG, E_NOTIMPL, HWND, POINT, RECT, S_OK};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, FORMATETC,
    IClassFactory, IDataObject,
};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyboardState, MAPVK_VK_TO_VSC, MapVirtualKeyW, SetKeyboardState, VK_CAPITAL, VK_SHIFT,
};
use windows::Win32::UI::TextServices::*;
use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage};
use windows_core::{BOOL, GUID, HRESULT, HSTRING, IUnknown, Interface, PCWSTR, PWSTR, Ref, Result, implement, s};

const CLSID_SWEET_IME: GUID = GUID::from_u128(0x7e1d3c2a_5b8f_4c1e_9a3d_2f6b8c4e1a01);

#[implement(ITextStoreACP)]
struct TextStore {
    text: RefCell<Vec<u16>>,
    sel: Cell<(i32, i32)>,
    sink: RefCell<Option<ITextStoreACPSink>>,
    reject_sync: Cell<bool>,
    layout_missing: Cell<bool>,
    /// The "application" window the document lives in.
    hwnd: HWND,
}

impl TextStore {
    fn len(&self) -> i32 {
        self.text.borrow().len() as i32
    }
}

impl ITextStoreACP_Impl for TextStore_Impl {
    fn AdviseSink(&self, _riid: *const GUID, punk: Ref<IUnknown>, _mask: u32) -> Result<()> {
        *self.sink.borrow_mut() = Some(punk.ok()?.cast()?);
        Ok(())
    }
    fn UnadviseSink(&self, _punk: Ref<IUnknown>) -> Result<()> {
        *self.sink.borrow_mut() = None;
        Ok(())
    }
    fn RequestLock(&self, flags: u32) -> Result<HRESULT> {
        if self.reject_sync.get() && flags & TS_LF_SYNC != 0 {
            return Ok(TS_E_SYNCHRONOUS);
        }
        let sink = self.sink.borrow().clone().ok_or(windows_core::Error::from(E_INVALIDARG))?;
        unsafe { sink.OnLockGranted(TEXT_STORE_LOCK_FLAGS(flags & !TS_LF_SYNC))? };
        Ok(S_OK)
    }
    fn GetStatus(&self) -> Result<TS_STATUS> {
        Ok(TS_STATUS::default())
    }
    fn QueryInsert(&self, s: i32, e: i32, _cch: u32, rs: *mut i32, re: *mut i32) -> Result<()> {
        unsafe {
            *rs = s;
            *re = e;
        }
        Ok(())
    }
    fn GetSelection(&self, _idx: u32, count: u32, out: *mut TS_SELECTION_ACP, fetched: *mut u32) -> Result<()> {
        if count == 0 {
            return Ok(());
        }
        let (s, e) = self.sel.get();
        unsafe {
            *out = TS_SELECTION_ACP {
                acpStart: s,
                acpEnd: e,
                style: TS_SELECTIONSTYLE { ase: TS_AE_END, fInterimChar: BOOL(0) },
            };
            *fetched = 1;
        }
        Ok(())
    }
    fn SetSelection(&self, count: u32, sel: *const TS_SELECTION_ACP) -> Result<()> {
        if count > 0 {
            let s = unsafe { &*sel };
            self.sel.set((s.acpStart, s.acpEnd));
        }
        Ok(())
    }
    fn GetText(
        &self, start: i32, end: i32, pch: PWSTR, cchreq: u32, cchret: *mut u32,
        runs: *mut TS_RUNINFO, runreq: u32, runret: *mut u32, next: *mut i32,
    ) -> Result<()> {
        let t = self.text.borrow();
        let end = if end < 0 { t.len() as i32 } else { end.min(t.len() as i32) };
        let n = ((end - start).max(0) as u32).min(cchreq);
        unsafe {
            if n > 0 && !pch.is_null() {
                std::ptr::copy_nonoverlapping(t[start as usize..].as_ptr(), pch.0, n as usize);
            }
            *cchret = n;
            if runreq > 0 && !runs.is_null() {
                *runs = TS_RUNINFO { uCount: n, r#type: TS_RT_PLAIN };
                *runret = 1;
            } else if !runret.is_null() {
                *runret = 0;
            }
            *next = start + n as i32;
        }
        Ok(())
    }
    fn SetText(&self, _flags: u32, start: i32, end: i32, pch: &PCWSTR, cch: u32) -> Result<TS_TEXTCHANGE> {
        let new: &[u16] = if cch == 0 { &[] } else { unsafe { std::slice::from_raw_parts(pch.0, cch as usize) } };
        self.text.borrow_mut().splice(start as usize..end as usize, new.iter().copied());
        let ne = start + cch as i32;
        self.sel.set((ne, ne));
        Ok(TS_TEXTCHANGE { acpStart: start, acpOldEnd: end, acpNewEnd: ne })
    }
    fn GetFormattedText(&self, _s: i32, _e: i32) -> Result<IDataObject> {
        Err(E_NOTIMPL.into())
    }
    fn GetEmbedded(&self, _p: i32, _g: *const GUID, _r: *const GUID) -> Result<IUnknown> {
        Err(E_NOTIMPL.into())
    }
    fn QueryInsertEmbedded(&self, _g: *const GUID, _f: *const FORMATETC) -> Result<BOOL> {
        Ok(BOOL(0))
    }
    fn InsertEmbedded(&self, _f: u32, _s: i32, _e: i32, _d: Ref<IDataObject>) -> Result<TS_TEXTCHANGE> {
        Err(E_NOTIMPL.into())
    }
    fn InsertTextAtSelection(
        &self, flags: u32, pch: &PCWSTR, cch: u32, ps: *mut i32, pe: *mut i32, change: *mut TS_TEXTCHANGE,
    ) -> Result<()> {
        let (s, e) = self.sel.get();
        if flags & TS_IAS_QUERYONLY != 0 {
            unsafe {
                *ps = s;
                *pe = e;
            }
            return Ok(());
        }
        let c = self.SetText(0, s, e, pch, cch)?;
        unsafe {
            if !ps.is_null() {
                *ps = s;
                *pe = c.acpNewEnd;
            }
            if !change.is_null() {
                *change = c;
            }
        }
        Ok(())
    }
    fn InsertEmbeddedAtSelection(&self, _f: u32, _d: Ref<IDataObject>, _s: *mut i32, _e: *mut i32, _c: *mut TS_TEXTCHANGE) -> Result<()> {
        Err(E_NOTIMPL.into())
    }
    fn RequestSupportedAttrs(&self, _f: u32, _c: u32, _a: *const GUID) -> Result<()> {
        Ok(())
    }
    fn RequestAttrsAtPosition(&self, _p: i32, _c: u32, _a: *const GUID, _f: u32) -> Result<()> {
        Ok(())
    }
    fn RequestAttrsTransitioningAtPosition(&self, _p: i32, _c: u32, _a: *const GUID, _f: u32) -> Result<()> {
        Ok(())
    }
    fn FindNextAttrTransition(
        &self, _s: i32, halt: i32, _c: u32, _a: *const GUID, _f: u32, next: *mut i32, found: *mut BOOL, off: *mut i32,
    ) -> Result<()> {
        unsafe {
            *next = halt;
            *found = BOOL(0);
            *off = 0;
        }
        Ok(())
    }
    fn RetrieveRequestedAttrs(&self, _c: u32, _v: *mut TS_ATTRVAL, fetched: *mut u32) -> Result<()> {
        unsafe { *fetched = 0 };
        Ok(())
    }
    fn GetEndACP(&self) -> Result<i32> {
        Ok(self.len())
    }
    fn GetActiveView(&self) -> Result<u32> {
        Ok(1)
    }
    fn GetACPFromPoint(&self, _v: u32, _p: *const POINT, _f: u32) -> Result<i32> {
        Err(E_NOTIMPL.into())
    }
    fn GetTextExt(&self, _v: u32, s: i32, e: i32, rc: *mut RECT, clipped: *mut BOOL) -> Result<()> {
        if self.layout_missing.get() {
            return Err(windows_core::Error::from(TS_E_NOLAYOUT));
        }
        unsafe {
            *rc = RECT { left: 200 + s * 16, top: 200, right: 200 + e * 16, bottom: 224 };
            *clipped = BOOL(0);
        }
        Ok(())
    }
    fn GetScreenExt(&self, _v: u32) -> Result<RECT> {
        Ok(RECT { left: 0, top: 0, right: 800, bottom: 600 })
    }
    fn GetWnd(&self, _v: u32) -> Result<HWND> {
        Ok(self.hwnd)
    }
}

type GetClassObject = unsafe extern "system" fn(*const GUID, *const GUID, *mut *mut c_void) -> HRESULT;

fn pump() {
    unsafe {
        let mut msg = MSG::default();
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

struct Harness {
    sink: ITfKeyEventSink,
    ctx: ITfContext,
    store: windows_core::ComObject<TextStore>,
}

impl Harness {
    fn key(&self, vk: u16) -> bool {
        unsafe {
            let scan = MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC);
            let lp = windows::Win32::Foundation::LPARAM(((scan as isize) << 16) | 1);
            let wp = windows::Win32::Foundation::WPARAM(vk as usize);
            let test = self.sink.OnTestKeyDown(&self.ctx, wp, lp).map(|b| b.as_bool()).unwrap_or(false);
            let eaten = test && self.sink.OnKeyDown(&self.ctx, wp, lp).map(|b| b.as_bool()).unwrap_or(false);
            pump();
            eaten
        }
    }
    fn type_str(&self, s: &str) {
        for c in s.chars() {
            let vk = match c {
                'a'..='z' => c.to_ascii_uppercase() as u16,
                '-' => 0xBD, // VK_OEM_MINUS
                _ => panic!("unsupported char {c}"),
            };
            assert!(self.key(vk), "key {c} not eaten");
        }
    }
    fn text(&self) -> String {
        String::from_utf16_lossy(&self.store.text.borrow())
    }
    fn clear(&self) {
        self.store.text.borrow_mut().clear();
        self.store.sel.set((0, 0));
    }
}

/// Saves the candidate window's screen area as a 32-bit BMP.
fn screenshot(path: &str) {
    screenshot_of(own_candidate_window(), path)
}

/// Saves a window's screen area (plus a margin) as a 32-bit BMP.
fn screenshot_of(hwnd: Option<HWND>, path: &str) {
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;
    unsafe {
        let Some(hwnd) = hwnd else {
            println!("FAIL screenshot: window not found");
            return;
        };
        let mut rc = RECT::default();
        let _ = GetWindowRect(hwnd, &mut rc);
        let (m, w, hgt) = (16, rc.right - rc.left + 32, rc.bottom - rc.top + 32);
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let bmp = CreateCompatibleBitmap(screen, w, hgt);
        let old = SelectObject(mem, bmp.into());
        let _ = BitBlt(mem, 0, 0, w, hgt, Some(screen), rc.left - m, rc.top - m, SRCCOPY);
        let mut bi = BITMAPINFO::default();
        bi.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: hgt,
            biPlanes: 1,
            biBitCount: 32,
            ..Default::default()
        };
        let mut px = vec![0u8; (w * hgt * 4) as usize];
        GetDIBits(mem, bmp, 0, hgt as u32, Some(px.as_mut_ptr() as *mut _), &mut bi, DIB_RGB_COLORS);
        SelectObject(mem, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        let mut f = Vec::new();
        let size = 14 + 40 + px.len() as u32;
        f.extend_from_slice(b"BM");
        f.extend_from_slice(&size.to_le_bytes());
        f.extend_from_slice(&0u32.to_le_bytes());
        f.extend_from_slice(&54u32.to_le_bytes());
        let hdr = std::slice::from_raw_parts(&bi.bmiHeader as *const _ as *const u8, 40);
        f.extend_from_slice(hdr);
        f.extend_from_slice(&px);
        std::fs::write(path, f).unwrap();
        println!("     screenshot {}x{} -> {path}", w, hgt);
    }
}

/// The candidate window of this process (an installed IME may have others
/// in other processes).
fn own_candidate_window() -> Option<HWND> {
    own_window(windows_core::w!("SweetImeCandidateWindow"))
}

/// Module handle of the DLL under test. An installed Sweet IME may also be
/// loaded into this process (by the system) with windows of the same
/// classes; only windows created by the tested DLL count.
static TESTED_MODULE: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

fn own_window(class: PCWSTR) -> Option<HWND> {
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowExW, GWLP_HINSTANCE, GetWindowLongPtrW, GetWindowThreadProcessId,
    };
    let pid = std::process::id();
    let module = TESTED_MODULE.load(std::sync::atomic::Ordering::Relaxed);
    let mut after: Option<HWND> = None;
    unsafe {
        loop {
            let h = FindWindowExW(None, after, class, None).ok()?;
            let mut p = 0;
            GetWindowThreadProcessId(h, Some(&mut p));
            if p == pid && (module == 0 || GetWindowLongPtrW(h, GWLP_HINSTANCE) == module) {
                return Some(h);
            }
            after = Some(h);
        }
    }
}

fn check(name: &str, got: String, want: &str, fails: &mut u32) {
    if got == want {
        println!("ok   {name}: {got}");
    } else {
        println!("FAIL {name}: got {got:?}, want {want:?}");
        *fails += 1;
    }
}

/// The entry point DLL (`sweet_tsf.dll`) switches a running text service
/// to the core that `core.txt` names when the thread gets focus again.
unsafe fn hot_swap(tm: &ITfThreadMgr, tid: u32, core: &str, appdata: &std::path::Path, fails: &mut u32) -> Result<()> {
    use windows::Win32::UI::TextServices::{ITfFunction, ITfThreadFocusSink};
    let core = std::path::Path::new(core);
    let dir = core.parent().unwrap();
    let shim = dir.join("sweet_tsf.dll");
    if !shim.exists() {
        println!("skip hot swap ({} not built)", shim.display());
        return Ok(());
    }
    // The copies don't sit next to the dictionary: point them at it.
    if std::env::var_os("SWEET_IME_DICT").is_none() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let dict = [dir.join("system.dict"), dir.join("../system.dict"), dir.join("data/system.dict"), root.join("data/system.dict")]
            .into_iter()
            .find(|p| p.exists());
        if let Some(d) = dict {
            unsafe { std::env::set_var("SWEET_IME_DICT", d) };
        }
    }
    let td = appdata.join("swap");
    std::fs::create_dir_all(&td).unwrap();
    std::fs::copy(&shim, td.join("sweet_tsf.dll")).unwrap();
    for name in ["sweet_tsf_core-a.dll", "sweet_tsf_core-b.dll"] {
        std::fs::copy(core, td.join(name)).unwrap();
    }
    std::fs::write(td.join("core.txt"), "sweet_tsf_core-a.dll\n").unwrap();
    type CoreName = unsafe extern "system" fn(*mut u16, u32) -> u32;
    unsafe {
        let module = LoadLibraryW(&HSTRING::from(td.join("sweet_tsf.dll").as_os_str()))?;
        let get: GetClassObject = std::mem::transmute(GetProcAddress(module, s!("DllGetClassObject")).unwrap());
        let core_name: CoreName = std::mem::transmute(GetProcAddress(module, s!("SweetShimCoreName")).unwrap());
        let name = || {
            let mut buf = [0u16; 128];
            let n = core_name(buf.as_mut_ptr(), 128) as usize;
            String::from_utf16_lossy(&buf[..n.min(128)])
        };
        let mut factory: *mut c_void = std::ptr::null_mut();
        get(&CLSID_SWEET_IME, &IClassFactory::IID, &mut factory).ok()?;
        let factory = IClassFactory::from_raw(factory);
        let tip: ITfTextInputProcessor = factory.CreateInstance(None)?;
        tip.Activate(tm, tid)?;
        pump();
        check("shim loads the named core", name(), "sweet_tsf_core-a.dll", fails);
        check("shim forwards settings name", tip.cast::<ITfFunction>()?.GetDisplayName()?.to_string(), "Sweet IME の設定", fails);
        // An update installs core b; focus returns to the application.
        std::fs::write(td.join("core.txt"), "sweet_tsf_core-b.dll\n").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2100));
        tip.cast::<ITfThreadFocusSink>()?.OnSetThreadFocus()?;
        let t = std::time::Instant::now();
        while name() != "sweet_tsf_core-b.dll" && t.elapsed() < std::time::Duration::from_secs(2) {
            std::thread::sleep(std::time::Duration::from_millis(10));
            pump();
        }
        check("focus switches to the new core", name(), "sweet_tsf_core-b.dll", fails);
        check("new core answers", tip.cast::<ITfFunction>()?.GetDisplayName()?.to_string(), "Sweet IME の設定", fails);
        tip.Deactivate()?;
    }
    Ok(())
}

fn main() -> Result<()> {
    let dll = std::env::args().nth(1).unwrap_or_else(|| {
        concat!(env!("CARGO_MANIFEST_DIR"), "\\..\\..\\target\\release\\sweet_tsf_core.dll").to_string()
    });
    // Keep learning history out of the real profile.
    let appdata = std::env::temp_dir().join(format!("sweet-ime-harness-{}", std::process::id()));
    unsafe { std::env::set_var("APPDATA", &appdata) };
    // Optional theme for screenshots (SWEET_HARNESS_THEME=sakura).
    if let Ok(theme) = std::env::var("SWEET_HARNESS_THEME") {
        let o = sweet_engine::Options {
            theme: sweet_engine::Theme::from_id(&theme).expect("unknown SWEET_HARNESS_THEME"),
            ..Default::default()
        };
        sweet_engine::paths::save_options(&o).expect("write config.txt");
    }
    // Custom key bindings: Ctrl+Space toggles, 変換 converts while composing.
    let preserved = {
        use sweet_engine::keymap::{Action, Binding, Chord, When};
        let mut km = sweet_engine::Keymap::default();
        km.bindings.push(Binding {
            chord: Chord { vk: 0x20, ctrl: true, shift: false, alt: false },
            action: Action::ImeToggle,
            when: When::Always,
        });
        km.bindings.push(Binding { chord: Chord::key(0x1C), action: Action::Key(sweet_engine::Key::Space), when: When::Composing });
        std::fs::create_dir_all(appdata.join("SweetIME")).unwrap();
        std::fs::write(appdata.join("SweetIME").join("keymap.txt"), km.to_text()).unwrap();
        sweet_engine::keymap::preserved_toggles(&km)
    };
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        let module = LoadLibraryW(&HSTRING::from(dll.as_str()))?;
        TESTED_MODULE.store(module.0 as isize, std::sync::atomic::Ordering::Relaxed);
        let proc = GetProcAddress(module, s!("DllGetClassObject")).expect("DllGetClassObject");
        let get: GetClassObject = std::mem::transmute(proc);
        let mut factory: *mut c_void = std::ptr::null_mut();
        get(&CLSID_SWEET_IME, &IClassFactory::IID, &mut factory).ok()?;
        let factory = IClassFactory::from_raw(factory);
        let tip: ITfTextInputProcessor = factory.CreateInstance(None)?;
        eprintln!("step: tip created");

        let tm: ITfThreadMgr = CoCreateInstance(&CLSID_TF_ThreadMgr, None, CLSCTX_INPROC_SERVER)?;
        let tid = tm.Activate()?;
        eprintln!("step: tm activated {tid}");
        let dim = tm.CreateDocumentMgr()?;
        let app_hwnd = windows::Win32::UI::WindowsAndMessaging::CreateWindowExW(
            Default::default(),
            windows_core::w!("STATIC"),
            windows_core::w!("harness app"),
            windows::Win32::UI::WindowsAndMessaging::WS_OVERLAPPEDWINDOW,
            0, 0, 800, 600, None, None, None, None,
        )?;
        let store = windows_core::ComObject::new(TextStore {
            text: RefCell::new(Vec::new()),
            sel: Cell::new((0, 0)),
            sink: RefCell::new(None),
            reject_sync: Cell::new(false),
            layout_missing: Cell::new(false),
            hwnd: app_hwnd,
        });
        let store_unk: IUnknown = store.to_interface();
        let mut ctx = None;
        let mut ec = 0;
        dim.CreateContext(tid, 0, &store_unk, &mut ctx, &mut ec)?;
        eprintln!("step: context");
        let ctx = ctx.unwrap();
        dim.Push(&ctx)?;
        tm.SetFocus(&dim)?;
        eprintln!("step: focus");

        let t0 = std::time::Instant::now();
        let tip_tid = tm.cast::<ITfClientId>()?.GetClientId(&CLSID_SWEET_IME);
        eprintln!("step: client id {tip_tid:?}");
        let tip_tid = tip_tid.unwrap_or(tid);
        tip.Activate(&tm, tip_tid)?;
        println!("activated in {:.2?}", t0.elapsed());
        // A real host runs its message loop between activation and typing.
        pump();
        // Windows language settings open the options through ITfFnConfigure.
        let configure: windows::Win32::UI::TextServices::ITfFnConfigure = tip.cast()?;
        let name = configure.GetDisplayName()?.to_string();
        if std::env::var_os("SWEET_TEST_CONFIGURE").is_some() {
            configure.Show(HWND::default(), 0x0411, &GUID::zeroed())?;
        }
        let h = Harness { sink: tip.cast()?, ctx: ctx.clone(), store };
        let mut fails = 0;
        check("configure name", name, "Sweet IME の設定", &mut fails);

        // 1. compose + convert + commit (twice: the first run includes
        // one-time candidate window creation)
        for run in 0..2 {
            h.clear();
            let t = std::time::Instant::now();
            h.type_str("kyouhaiitenkidesune");
            check("live preedit", h.text(), "今日はいい天気ですね", &mut fails);
            h.key(0x20); // space
            check("converted", h.text(), "今日はいい天気ですね", &mut fails);
            h.key(0x0D); // enter
            check("committed", h.text(), "今日はいい天気ですね", &mut fails);
            println!("     round trip #{run} {:.2?}", t.elapsed());
        }

        // 1a. live conversion is refined in the background by the reranker
        h.clear();
        let t = std::time::Instant::now();
        h.type_str("kinounokaiginoshiryou");
        let typed = t.elapsed();
        let lattice = h.text();
        let mut refined = lattice.clone();
        let t = std::time::Instant::now();
        while t.elapsed() < std::time::Duration::from_secs(2) {
            std::thread::sleep(std::time::Duration::from_millis(5));
            pump();
            refined = h.text();
            if refined != lattice {
                break;
            }
        }
        println!("     typed in {typed:.2?}; lattice {lattice}; refined after {:.2?}: {refined}", t.elapsed());
        check("live refined", refined.clone(), "昨日の会議の資料", &mut fails);
        let t = std::time::Instant::now();
        h.key(0x20);
        println!("     space after refinement {:.2?}", t.elapsed());
        check("space keeps refined", h.text(), "昨日の会議の資料", &mut fails);
        h.key(0x0D);

        // 1b. Esc during live conversion reveals the reading
        h.clear();
        h.type_str("kyouhaiitenkidesune");
        h.key(0x1B);
        check("reading after esc", h.text(), "きょうはいいてんきですね", &mut fails);
        h.key(0x1B);
        check("cancel after 2nd esc", h.text(), "", &mut fails);

        // 2. candidate window + selection by digit
        h.clear();
        h.type_str("kisha");
        h.key(0x20);
        h.key(0x20);
        h.key(0x20);
        let shown = h.text();
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(10));
            pump();
        }
        if let Ok(path) = std::env::var("SWEET_SHOT") {
            screenshot(&path);
        }
        h.key(0x0D);
        check("candidate", h.text(), &shown, &mut fails);

        // 2b. left context steers conversion (needs zenz.swz next to the DLL)
        for (ctx, input, want) in [("明日は音楽の", "kouenn", "公演"), ("近所の", "kouenn", "公園")] {
            h.clear();
            let pre: Vec<u16> = ctx.encode_utf16().collect();
            *h.store.text.borrow_mut() = pre.clone();
            h.store.sel.set((pre.len() as i32, pre.len() as i32));
            h.type_str(input);
            h.key(0x20);
            h.key(0x0D);
            check(&format!("context {ctx}"), h.text(), &format!("{ctx}{want}"), &mut fails);
        }

        // 2b'. mouse click on the third candidate commits it
        h.clear();
        h.type_str("kisha");
        h.key(0x20);
        h.key(0x20);
        pump();
        {
            use windows::Win32::UI::HiDpi::GetDpiForWindow;
            use windows::Win32::UI::WindowsAndMessaging::{SendMessageW, WM_LBUTTONUP};
            let hwnd = own_candidate_window().expect("candidate window in this process");
            let owner = windows::Win32::UI::WindowsAndMessaging::GetWindow(hwnd, windows::Win32::UI::WindowsAndMessaging::GW_OWNER)
                .unwrap_or_default();
            check("candidate window owner", format!("{:?}", owner.0), &format!("{:?}", app_hwnd.0), &mut fails);
            let scale = GetDpiForWindow(hwnd).max(96) as f32 / 96.0;
            // Rows start at PAD=4 and are ROW_H=32 tall (logical px).
            let y = ((4.0 + 2.5 * 32.0) * scale) as isize;
            let x = (40.0 * scale) as isize;
            let before = h.text();
            SendMessageW(hwnd, WM_LBUTTONUP, Some(windows::Win32::Foundation::WPARAM(0)),
                Some(windows::Win32::Foundation::LPARAM((y << 16) | x)));
            pump();
            let after = h.text();
            // After a commit the TIP no longer wants Backspace.
            let composing = h
                .sink
                .OnTestKeyDown(&h.ctx, windows::Win32::Foundation::WPARAM(0x08), windows::Win32::Foundation::LPARAM(1))
                .map(|b| b.as_bool())
                .unwrap_or(true);
            if after == before || composing {
                println!("FAIL click: before {before:?} after {after:?} composing {composing}");
                fails += 1;
            } else {
                println!("ok   click: {before} -> {after}");
            }
        }

        // Some editors publish text geometry only after the edit completes.
        h.clear();
        h.store.layout_missing.set(true);
        h.type_str("kisha");
        h.key(0x20);
        h.key(0x20);
        pump();
        let hwnd = own_candidate_window().expect("candidate window in this process");
        use windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, IsWindowVisible};
        check("candidate hidden until layout", IsWindowVisible(hwnd).as_bool().to_string(), "false", &mut fails);
        h.store.layout_missing.set(false);
        let sink = h.store.sink.borrow().clone().unwrap();
        sink.OnLayoutChange(TS_LC_CHANGE, 1)?;
        pump();
        let mut rect = RECT::default();
        GetWindowRect(hwnd, &mut rect)?;
        let placed = IsWindowVisible(hwnd).as_bool() && (150..=205).contains(&rect.left) && rect.top == 228;
        check("candidate follows delayed layout", placed.to_string(), "true", &mut fails);
        h.key(0x0D);

        // 2c. suggestions while typing; Tab enters the list
        h.clear();
        h.type_str("arigat");
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(10));
            pump();
        }
        if let Ok(path) = std::env::var("SWEET_SHOT_SUGGEST") {
            screenshot(&path);
        }
        h.key(0x09); // tab
        let picked = h.text();
        h.key(0x0D);
        check("suggestion", h.text(), &picked, &mut fails);
        if picked.starts_with("ありがt") || picked.is_empty() {
            println!("FAIL suggestion not applied: {picked:?}");
            fails += 1;
        }

        // 3. escape cancels
        h.clear();
        h.type_str("aiueo");
        h.key(0x1B);
        h.key(0x1B);
        check("escape", h.text(), "", &mut fails);

        // 4. F7 katakana
        h.clear();
        h.type_str("ko-hi-");
        h.key(0x76); // F7
        h.key(0x0D);
        check("katakana", h.text(), "コーヒー", &mut fails);

        // 5. IME off passes keys through
        h.clear();
        h.key(0xF3); // 半角/全角
        let eaten = h.key(0x41);
        check("off passthrough", format!("{eaten}"), "false", &mut fails);
        // The mode badge appeared at the caret ("A").
        {
            use windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, IsWindowVisible};
            pump();
            let badge = own_window(windows_core::w!("SweetImeModeIndicator"));
            let visible = badge.is_some_and(|b| IsWindowVisible(b).as_bool());
            check("mode badge shown", format!("{visible}"), "true", &mut fails);
            if let Some(b) = badge {
                let mut rc = RECT::default();
                let _ = GetWindowRect(b, &mut rc);
                // The caret is at (200, 224) bottom in the fake text store.
                check("mode badge below caret", format!("{}", rc.top >= 224 && rc.top < 240), "true", &mut fails);
                if let Ok(dir) = std::env::var("SWEET_SHOT_DIR") {
                    screenshot_of(Some(b), &format!("{dir}\\badge-off.bmp"));
                }
            }
        }
        h.key(0xF3);
        pump();
        if let Ok(dir) = std::env::var("SWEET_SHOT_DIR") {
            screenshot_of(own_window(windows_core::w!("SweetImeModeIndicator")), &format!("{dir}\\badge-on.bmp"));
        }
        let eaten = h.key(0x41);
        check("on again", format!("{eaten}"), "true", &mut fails);
        // Typing hides the badge.
        {
            let visible = own_window(windows_core::w!("SweetImeModeIndicator"))
                .is_some_and(|b| windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(b).as_bool());
            check("mode badge hidden while typing", format!("{visible}"), "false", &mut fails);
        }
        h.key(0x1B);
        h.key(0x1B);

        // 6. custom key bindings (keymap.txt)
        h.clear();
        h.type_str("kinou");
        h.key(0x1C); // 変換 -> convert while composing
        let converted = h.store.text.borrow().len() > 0 && h.text() != "きのう";
        check("keymap henkan converts", format!("{converted}"), "true", &mut fails);
        h.key(0x1B);
        h.key(0x1B);
        h.key(0x1B);
        h.clear();
        let with_ctrl = |down: bool| {
            let mut ks = [0u8; 256];
            let _ = GetKeyboardState(&mut ks);
            ks[0x11] = if down { 0x80 } else { 0 };
            ks[0xA2] = ks[0x11];
            let _ = SetKeyboardState(&ks);
        };
        with_ctrl(true);
        let eaten = h.key(0x20);
        with_ctrl(false);
        check("keymap ctrl+space eaten", format!("{eaten}"), "true", &mut fails);
        let eaten = h.key(0x41);
        check("keymap ctrl+space turned off", format!("{eaten}"), "false", &mut fails);
        with_ctrl(true);
        h.key(0x20);
        with_ctrl(false);
        let eaten = h.key(0x41);
        check("keymap ctrl+space turned on", format!("{eaten}"), "true", &mut fails);
        h.key(0x1B);
        h.key(0x1B);

        // 7. Hotkeys delivered as preserved keys.
        h.clear();
        let key_guid = |wanted| {
            let index = preserved.iter().position(|(chord, _)| *chord == wanted).unwrap();
            GUID::from_u128(0x7e1d3c2a_5b8f_4c1e_9a3d_2f6b8c4e1b00 + index as u128)
        };
        use sweet_engine::keymap::Chord;
        let alt_grave = key_guid(Chord { alt: true, ..Chord::key(0xC0) });
        let eaten = h.sink.OnPreservedKey(&h.ctx, &alt_grave).map(|b| b.as_bool()).unwrap_or(false);
        pump();
        check("alt+` preserved key eaten", format!("{eaten}"), "true", &mut fails);
        check("alt+` turned off", format!("{}", h.key(0x41)), "false", &mut fails);
        let _ = h.sink.OnPreservedKey(&h.ctx, &alt_grave);
        check("alt+` turned on", format!("{}", h.key(0x41)), "true", &mut fails);
        h.key(0x1B);
        h.key(0x1B);

        h.clear();
        h.type_str("kana");
        let composed = h.text();
        h.store.reject_sync.set(true);
        let eisu = key_guid(Chord::key(0xF0));
        let hankaku = key_guid(Chord::key(0xF4));
        let eaten = h.sink.OnPreservedKey(None::<&ITfContext>, &eisu)?.as_bool();
        pump();
        h.store.reject_sync.set(false);
        check("英数 preserved without context", eaten.to_string(), "true", &mut fails);
        check("英数 turns off during composition", h.key(0x41).to_string(), "false", &mut fails);
        check("英数 keeps composed text", h.text(), &composed, &mut fails);
        let compositions = h.ctx.cast::<ITfContextComposition>()?.EnumCompositions()?;
        let mut fetched = 0;
        compositions.Next(&mut [None], &mut fetched)?;
        check("英数 ends the composition", fetched.to_string(), "0", &mut fails);
        let eaten = h.sink.OnPreservedKey(None::<&ITfContext>, &hankaku)?.as_bool();
        check("半角/全角 alias turns on", eaten.to_string(), "true", &mut fails);
        check("on types Japanese again", h.key(0x41).to_string(), "true", &mut fails);
        h.key(0x1B);
        h.key(0x1B);

        // Shift on the first letter starts a temporary English word.
        h.clear();
        let mut original = [0u8; 256];
        GetKeyboardState(&mut original)?;
        let mut keyboard = original;
        keyboard[VK_CAPITAL.0 as usize] = 0;
        keyboard[VK_SHIFT.0 as usize] = 0x80;
        SetKeyboardState(&keyboard)?;
        let eaten = h.key(b'T' as u16);
        keyboard[VK_SHIFT.0 as usize] = 0;
        SetKeyboardState(&keyboard)?;
        h.type_str("est");
        h.key(0x20);
        check(
            "shift starts English input",
            eaten.to_string(),
            "true",
            &mut fails,
        );
        check(
            "space commits the English word",
            h.text(),
            "Test ",
            &mut fails,
        );
        h.type_str("kana");
        check(
            "Japanese resumes after English",
            h.text(),
            "Test かな",
            &mut fails,
        );
        h.key(0x1B);
        h.key(0x1B);
        for (typed, expected) in [("ime", "IME"), ("ieee", "IEEE")] {
            h.clear();
            h.type_str(typed);
            h.key(0x20);
            h.key(0x0D);
            check("technical spelling", h.text(), expected, &mut fails);
        }
        SetKeyboardState(&original)?;
        // The taskbar mode indicator can only be added for a registered TIP.
        {
            use windows::Win32::UI::TextServices::{ITfLangBarItemButton, ITfLangBarItemMgr, TF_LANGBARITEMINFO};
            let mgr = tm.cast::<ITfLangBarItemMgr>()?;
            let en = mgr.EnumItems()?;
            let mut found = None;
            loop {
                let mut items = [None];
                let mut n = 0;
                if en.Next(&mut items, &mut n).is_err() || n == 0 {
                    break;
                }
                if let Some(item) = items[0].take() {
                    let mut info = TF_LANGBARITEMINFO::default();
                    if item.GetInfo(&mut info).is_ok() && info.clsidService == CLSID_SWEET_IME {
                        found = item.cast::<ITfLangBarItemButton>().ok();
                    }
                }
            }
            match found {
                Some(b) => check("mode indicator", b.GetText()?.to_string(), "あ", &mut fails),
                None => println!("skip mode indicator (TIP not registered)"),
            }
        }

        tip.Deactivate()?;

        // Per-application modes: this process is harness.exe.
        let exe = std::env::current_exe().ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()));
        let apps = appdata.join("SweetIME").join("apps.txt");
        let a = MapVirtualKeyW(u32::from(b'A'), MAPVK_VK_TO_VSC);
        let key_a = (windows::Win32::Foundation::WPARAM(usize::from(b'A')), windows::Win32::Foundation::LPARAM(((a as isize) << 16) | 1));
        for (mode, want_open) in [("english", false), ("off", false), ("normal", true)] {
            std::fs::write(&apps, format!("{} = {mode}\n", exe.as_deref().unwrap_or("harness.exe"))).unwrap();
            let t: ITfTextInputProcessor = factory.CreateInstance(None)?;
            t.Activate(&tm, tip_tid)?;
            pump();
            let sink: ITfKeyEventSink = t.cast()?;
            let wants = sink.OnTestKeyDown(&h.ctx, key_a.0, key_a.1)?.as_bool();
            check(&format!("app mode {mode}: typing converts"), wants.to_string(), &want_open.to_string(), &mut fails);
            // 半角/全角 (VK_KANJI, VK_OEM_AUTO / VK_OEM_ENLW) toggles the IME,
            // except where the IME is off for the application.
            let toggle = [0x19usize, 0xF3, 0xF4].iter().any(|&vk| {
                let wp = windows::Win32::Foundation::WPARAM(vk);
                sink.OnTestKeyDown(&h.ctx, wp, windows::Win32::Foundation::LPARAM(1)).is_ok_and(|b| b.as_bool())
            });
            check(&format!("app mode {mode}: 半角/全角 toggles"), toggle.to_string(), &(mode != "off").to_string(), &mut fails);
            t.Deactivate()?;
        }
        let _ = std::fs::remove_file(&apps);

        hot_swap(&tm, tip_tid, &dll, &appdata, &mut fails)?;
        tm.Deactivate()?;
        let _ = std::fs::remove_dir_all(&appdata);
        println!("{}", if fails == 0 { "ALL PASSED" } else { "FAILURES" });
        std::process::exit(fails as i32);
    }
}
