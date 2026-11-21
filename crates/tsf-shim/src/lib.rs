//! Sweet IME's registered TSF entry point (`sweet_tsf.dll`).
//!
//! Applications load this small DLL. It loads the core named in `core.txt`
//! next to it (`sweet_tsf_core-<version>.dll`; `sweet_tsf_core.dll` during
//! development) and forwards the text input processor to it. After an
//! update writes a new core and `core.txt`, running applications switch to
//! it the next time they get focus, without being restarted. See `shim.rs`.
//!
//! Register with `regsvr32 sweet_tsf.dll` (elevated).

#![cfg(windows)]
#![allow(non_snake_case)]

mod register;
mod shim;

use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI32, AtomicIsize, Ordering};

use windows::Win32::Foundation::{
    CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_POINTER, HINSTANCE, HMODULE, MAX_PATH, S_FALSE, S_OK,
};
use windows::Win32::System::Com::{IClassFactory, IClassFactory_Impl};
use windows::Win32::System::LibraryLoader::{DisableThreadLibraryCalls, GetModuleFileNameW};
use windows::Win32::System::SystemServices::DLL_PROCESS_ATTACH;
use windows_core::{BOOL, GUID, HRESULT, IUnknown, Interface, Ref, Result, implement};

pub const CLSID_SWEET_IME: GUID = GUID::from_u128(0x7e1d3c2a_5b8f_4c1e_9a3d_2f6b8c4e1a01);
pub const GUID_PROFILE: GUID = GUID::from_u128(0x7e1d3c2a_5b8f_4c1e_9a3d_2f6b8c4e1a02);
pub const DISPLAY_NAME: &str = "Sweet IME";
pub const LANGID_JAPANESE: u16 = 0x0411;

static DLL_INSTANCE: AtomicIsize = AtomicIsize::new(0);
static LOCKS: AtomicI32 = AtomicI32::new(0);

pub(crate) fn dll_path() -> PathBuf {
    let mut buf = [0u16; MAX_PATH as usize * 4];
    let module = HMODULE(DLL_INSTANCE.load(Ordering::Relaxed) as *mut c_void);
    let n = unsafe { GetModuleFileNameW(Some(module), &mut buf) } as usize;
    PathBuf::from(String::from_utf16_lossy(&buf[..n]))
}

pub(crate) fn lock_module(lock: bool) {
    if lock {
        LOCKS.fetch_add(1, Ordering::SeqCst);
    } else {
        LOCKS.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Runs `f`, turning panics into `E_UNEXPECTED` so they never unwind into
/// the host application.
pub(crate) fn guard<T>(f: impl FnOnce() -> Result<T>) -> Result<T> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(_) => Err(windows::Win32::Foundation::E_UNEXPECTED.into()),
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
    fn CreateInstance(&self, outer: Ref<IUnknown>, riid: *const GUID, ppv: *mut *mut c_void) -> Result<()> {
        if ppv.is_null() {
            return Err(E_POINTER.into());
        }
        unsafe { *ppv = std::ptr::null_mut() };
        if outer.is_some() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        let unk: IUnknown = shim::Shim::new().into();
        unsafe { unk.query(riid, ppv).ok() }
    }

    fn LockServer(&self, lock: BOOL) -> Result<()> {
        lock_module(lock.as_bool());
        Ok(())
    }
}

#[unsafe(no_mangle)]
unsafe extern "system" fn DllGetClassObject(rclsid: *const GUID, riid: *const GUID, ppv: *mut *mut c_void) -> HRESULT {
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
    // Loaded cores stay loaded; only this DLL's own objects count.
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

/// Writes the file name of the core loaded most recently into `buf`
/// (UTF-16, `len` units) and returns its length. For the test harness.
#[unsafe(no_mangle)]
unsafe extern "system" fn SweetShimCoreName(buf: *mut u16, len: u32) -> u32 {
    let name: Vec<u16> = shim::latest_core_name().encode_utf16().collect();
    if !buf.is_null() {
        let n = name.len().min(len as usize);
        unsafe { std::ptr::copy_nonoverlapping(name.as_ptr(), buf, n) };
    }
    name.len() as u32
}
