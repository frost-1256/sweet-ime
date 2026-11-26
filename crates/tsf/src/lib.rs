//! sweet-ime Text Services Framework (TSF) text input processor: the core.
//!
//! Applications load the registered entry point `sweet_tsf.dll`
//! (`crates/tsf-shim`), which loads this DLL (`sweet_tsf_core-<version>.dll`
//! when installed) and switches to a newer one after an update.

#![cfg(windows)]
#![allow(non_snake_case)]

mod candidate_window;
mod display_attr;
mod edit;
mod keys;
mod langbar;
mod mode_indicator;
mod service;

use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicI32, AtomicIsize, Ordering};

use sweet_engine::{Converter, Dictionary};
use windows::Win32::Foundation::{
    CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_POINTER, HINSTANCE, HMODULE, MAX_PATH,
    S_FALSE, S_OK,
};
use windows::Win32::System::Com::{IClassFactory, IClassFactory_Impl};
use windows::Win32::System::LibraryLoader::{DisableThreadLibraryCalls, GetModuleFileNameW};
use windows::Win32::System::SystemServices::DLL_PROCESS_ATTACH;
use windows_core::{BOOL, GUID, HRESULT, IUnknown, Interface, Ref, Result, implement};

pub const CLSID_SWEET_IME: GUID = GUID::from_u128(0x7e1d3c2a_5b8f_4c1e_9a3d_2f6b8c4e1a01);
pub const GUID_PROFILE: GUID = GUID::from_u128(0x7e1d3c2a_5b8f_4c1e_9a3d_2f6b8c4e1a02);
pub const GUID_ATTR_INPUT: GUID = GUID::from_u128(0x7e1d3c2a_5b8f_4c1e_9a3d_2f6b8c4e1a03);
pub const GUID_ATTR_CONVERTED: GUID = GUID::from_u128(0x7e1d3c2a_5b8f_4c1e_9a3d_2f6b8c4e1a04);
pub const GUID_ATTR_FOCUSED: GUID = GUID::from_u128(0x7e1d3c2a_5b8f_4c1e_9a3d_2f6b8c4e1a05);

/// Private `ActivateEx` flag from the entry point: this core replaces an
/// older one in a running application. Keep the input mode and stay quiet.
pub const ACTIVATE_HOT_SWAP: u32 = 0x4000_0000;

static DLL_INSTANCE: AtomicIsize = AtomicIsize::new(0);
static LOCKS: AtomicI32 = AtomicI32::new(0);

pub(crate) fn dll_instance() -> HINSTANCE {
    HINSTANCE(DLL_INSTANCE.load(Ordering::Relaxed) as *mut c_void)
}

pub(crate) fn dll_path() -> PathBuf {
    let mut buf = [0u16; MAX_PATH as usize * 4];
    let n = unsafe { GetModuleFileNameW(Some(HMODULE(dll_instance().0)), &mut buf) } as usize;
    PathBuf::from(String::from_utf16_lossy(&buf[..n]))
}

/// The converter is shared by every text service instance in the process.
/// The dictionary is memory-mapped, so pages are shared across processes too.
pub(crate) fn converter() -> Option<&'static Converter> {
    static CONV: OnceLock<Option<Converter>> = OnceLock::new();
    CONV.get_or_init(|| {
        let mut candidates = Vec::new();
        if let Some(dir) = dll_path().parent() {
            candidates.push(dir.join("system.dict"));
            // Install layout: <root>\x64\sweet_tsf.dll + <root>\system.dict
            if let Some(root) = dir.parent() {
                candidates.push(root.join("system.dict"));
            }
            candidates.push(dir.join("data").join("system.dict"));
        }
        if let Ok(p) = std::env::var("SWEET_IME_DICT") {
            candidates.insert(0, PathBuf::from(p));
        }
        let conv = candidates
            .into_iter()
            .find_map(|p| Dictionary::open(&p).ok())
            .map(Converter::new)?;
        let conv = sweet_engine::paths::with_user_data(conv);
        Some(match load_zenz() {
            Some(r) => conv.with_reranker(Box::new(r)),
            None => conv,
        })
    })
    .as_ref()
}

/// Opens the settings app (`sweet-settings.exe` in the install root, or
/// next to the DLL during development).
pub(crate) fn launch_settings() {
    let Some(dir) = dll_path().parent().map(|d| d.to_path_buf()) else { return };
    let mut candidates = vec![dir.join("sweet-settings.exe")];
    if let Some(root) = dir.parent() {
        candidates.insert(0, root.join("sweet-settings.exe"));
    }
    match candidates.into_iter().find(|p| p.exists()) {
        Some(exe) => {
            // Don't hand the host's standard handles to the new process.
            let r = std::process::Command::new(exe)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
            if let Err(e) = r {
                debug_log!("launch settings: {e}");
            }
        }
        None => debug_log!("sweet-settings.exe not found"),
    }
}

/// Per-user settings from `%APPDATA%\SweetIME\config.txt` (`key = value`).
/// Read on every activation, so edits apply the next time the IME is
/// activated in an application.
pub(crate) fn load_options() -> sweet_engine::Options {
    let o = sweet_engine::paths::load_options();
    set_theme(o.theme);
    if let Some(c) = converter() {
        c.apply_options(&o);
    }
    o
}

static THEME: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

fn set_theme(t: sweet_engine::Theme) {
    let i = sweet_engine::Theme::ALL.iter().position(|&x| x == t).unwrap_or(0);
    THEME.store(i as u8, std::sync::atomic::Ordering::Relaxed);
}

/// The theme from the last settings load.
pub(crate) fn theme() -> sweet_engine::Theme {
    let i = THEME.load(std::sync::atomic::Ordering::Relaxed) as usize;
    sweet_engine::Theme::ALL.get(i).copied().unwrap_or_default()
}

/// Colors of the current theme for the current Windows app theme.
pub(crate) fn theme_colors() -> sweet_engine::theme::Colors {
    let t = theme();
    if t == sweet_engine::Theme::Custom {
        sweet_engine::paths::load_user_theme().colors()
    } else {
        t.colors(candidate_window::system_light())
    }
}

pub(crate) type SettingsStamp = [Option<std::time::SystemTime>; 5];

/// Modification times of the settings files; a change means the settings
/// app saved something and the service should reload.
pub(crate) fn settings_stamp() -> SettingsStamp {
    [
        sweet_engine::paths::modified(sweet_engine::paths::config_file()),
        sweet_engine::paths::modified(sweet_engine::paths::keymap_file()),
        sweet_engine::paths::modified(sweet_engine::paths::apps_file()),
        sweet_engine::paths::modified(sweet_engine::paths::candidate_font_file()),
        sweet_engine::paths::modified(sweet_engine::paths::user_theme_file()),
    ]
}

/// This application's mode from the per-application settings (the text
/// service runs inside the application's process).
pub(crate) fn app_mode() -> sweet_engine::app_modes::AppMode {
    let exe = std::env::current_exe().ok();
    let exe = exe.as_ref().and_then(|p| p.file_name()).and_then(|n| n.to_str()).unwrap_or("");
    sweet_engine::paths::load_app_modes().mode_for(exe)
}

/// Optional neural reranker. Not bundled (the model is CC BY-SA 4.0); the
/// user installs it with get-zenz.ps1 into `%LOCALAPPDATA%\SweetIME`.
/// Also looked up via `SWEET_IME_ZENZ` and next to the DLL (development).
fn load_zenz() -> Option<sweet_zenz::ZenzReranker> {
    let mut paths = Vec::new();
    if let Ok(p) = std::env::var("SWEET_IME_ZENZ") {
        paths.push(PathBuf::from(p));
    }
    if let Some(p) = sweet_engine::paths::zenz_file() {
        paths.push(p);
    }
    if let Some(dir) = dll_path().parent() {
        paths.push(dir.join("zenz.swz"));
        if let Some(root) = dir.parent() {
            paths.push(root.join("zenz.swz"));
        }
        paths.push(dir.join("data").join("zenz.swz"));
    }
    paths.into_iter().find_map(|p| sweet_zenz::Zenz::load(&p).ok()).map(sweet_zenz::ZenzReranker::new)
}

pub(crate) fn lock_module(lock: bool) {
    if lock {
        LOCKS.fetch_add(1, Ordering::SeqCst);
    } else {
        LOCKS.fetch_sub(1, Ordering::SeqCst);
    }
}

#[unsafe(no_mangle)]
extern "system" fn DllMain(hinst: HINSTANCE, reason: u32, _reserved: *mut c_void) -> BOOL {
    if reason == DLL_PROCESS_ATTACH {
        DLL_INSTANCE.store(hinst.0 as isize, Ordering::Relaxed);
        unsafe {
            let _ = DisableThreadLibraryCalls(HMODULE(hinst.0));
        }
    }
    BOOL(1)
}

#[implement(IClassFactory)]
struct ClassFactory;

impl IClassFactory_Impl for ClassFactory_Impl {
    fn CreateInstance(
        &self,
        outer: Ref<IUnknown>,
        riid: *const GUID,
        ppv: *mut *mut c_void,
    ) -> Result<()> {
        if ppv.is_null() {
            return Err(E_POINTER.into());
        }
        unsafe { *ppv = std::ptr::null_mut() };
        if outer.is_some() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        let unk: IUnknown = service::TextService::new().into();
        unsafe { unk.query(riid, ppv).ok() }
    }

    fn LockServer(&self, lock: BOOL) -> Result<()> {
        lock_module(lock.as_bool());
        Ok(())
    }
}

#[unsafe(no_mangle)]
unsafe extern "system" fn DllGetClassObject(
    rclsid: *const GUID,
    riid: *const GUID,
    ppv: *mut *mut c_void,
) -> HRESULT {
    if ppv.is_null() || rclsid.is_null() {
        return E_POINTER;
    }
    unsafe {
        *ppv = std::ptr::null_mut();
        if *rclsid != CLSID_SWEET_IME {
            return CLASS_E_CLASSNOTAVAILABLE;
        }
        let factory: IClassFactory = ClassFactory.into();
        factory.query(riid, ppv)
    }
}

#[unsafe(no_mangle)]
extern "system" fn DllCanUnloadNow() -> HRESULT {
    if LOCKS.load(Ordering::SeqCst) == 0 { S_OK } else { S_FALSE }
}

thread_local! {
    static BUSY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Records whether the text service on this thread has a composition.
pub(crate) fn set_busy(busy: bool) {
    BUSY.with(|b| b.set(busy));
}

/// Whether text is being composed on the calling thread. The entry point
/// asks before replacing this core with a newer one.
#[unsafe(no_mangle)]
extern "system" fn SweetCoreBusy() -> BOOL {
    BOOL(BUSY.with(|b| b.get()) as i32)
}

pub(crate) fn debug_enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("SWEET_IME_DEBUG").is_some())
}

/// Debug log to stderr and the debugger when `SWEET_IME_DEBUG` is set.
macro_rules! debug_log {
    ($($t:tt)*) => {
        if $crate::debug_enabled() {
            let s = format!("[sweet-ime] {}
", format!($($t)*));
            eprint!("{s}");
            let w: Vec<u16> = s.encode_utf16().chain(Some(0)).collect();
            #[allow(unused_unsafe)]
            unsafe { windows::Win32::System::Diagnostics::Debug::OutputDebugStringW(windows_core::PCWSTR(w.as_ptr())) };
        }
    };
}
pub(crate) use debug_log;

/// Run `f`, converting panics into `E_UNEXPECTED` so they never unwind
/// across the COM boundary into the host application.
pub(crate) fn guard<T>(f: impl FnOnce() -> Result<T>) -> Result<T> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(_) => Err(windows::Win32::Foundation::E_UNEXPECTED.into()),
    }
}
