//! sweet-ime Text Services Framework (TSF) text input processor.
//!
//! Build as a cdylib and register with `regsvr32 sweet_tsf.dll` (elevated).

#![cfg(windows)]
#![allow(non_snake_case)]

mod candidate_window;
mod display_attr;
mod edit;
mod keys;
mod register;
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

pub const DISPLAY_NAME: &str = "Sweet IME";
pub const LANGID_JAPANESE: u16 = 0x0411;

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
        let mut conv = candidates.into_iter().find_map(|p| Dictionary::open(&p).ok()).map(Converter::new)?;
        if let Some(dir) = std::env::var_os("APPDATA").map(PathBuf::from) {
            conv = conv.with_history(sweet_engine::UserHistory::with_file(dir.join("SweetIME").join("history.tsv")));
        }
        Some(match load_zenz() {
            Some(r) => conv.with_reranker(Box::new(r)),
            None => conv,
        })
    })
    .as_ref()
}

/// Per-user settings from `%APPDATA%\SweetIME\config.txt` (`key = value`).
/// Read on every activation, so edits apply the next time the IME is
/// activated in an application.
pub(crate) fn load_options() -> sweet_engine::Options {
    std::env::var_os("APPDATA")
        .map(|d| PathBuf::from(d).join("SweetIME").join("config.txt"))
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|t| sweet_engine::Options::parse(&t))
        .unwrap_or_default()
}

/// Optional neural reranker: `zenz.swz` next to the DLL (or `SWEET_IME_ZENZ`).
fn load_zenz() -> Option<sweet_zenz::ZenzReranker> {
    let mut paths = Vec::new();
    if let Ok(p) = std::env::var("SWEET_IME_ZENZ") {
        paths.push(PathBuf::from(p));
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

#[unsafe(no_mangle)]
extern "system" fn DllRegisterServer() -> HRESULT {
    match register::register() {
        Ok(()) => S_OK,
        Err(e) => {
            let _ = register::unregister();
            e.code()
        }
    }
}

#[unsafe(no_mangle)]
extern "system" fn DllUnregisterServer() -> HRESULT {
    match register::unregister() {
        Ok(()) => S_OK,
        Err(e) => e.code(),
    }
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
