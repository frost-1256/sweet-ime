//! The text input processor object that applications see. It forwards to
//! a text service created by a core DLL, and replaces that text service with
//! one from a newer core when the application gets focus again.
//!
//! The core registers its own key, focus and layout sinks with the thread
//! manager, so only the interfaces TSF asks of the registered CLSID itself
//! are forwarded here: activation, display attributes and the settings
//! entry in Windows language settings.
//!
//! An old core is never unloaded: its threads and windows may still be
//! running. Memory-mapped data (dictionary, model) is shared by file, so a
//! second core mostly costs its code.

use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{E_FAIL, HWND};
use windows::Win32::System::Com::IClassFactory;
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_WITH_ALTERED_SEARCH_PATH, LoadLibraryExW,
};
use windows::Win32::UI::TextServices::{
    IEnumTfDisplayAttributeInfo, ITfDisplayAttributeInfo, ITfDisplayAttributeProvider,
    ITfDisplayAttributeProvider_Impl, ITfFnConfigure, ITfFnConfigure_Impl, ITfFunction,
    ITfFunction_Impl, ITfSource, ITfTextInputProcessor, ITfTextInputProcessor_Impl,
    ITfTextInputProcessorEx, ITfTextInputProcessorEx_Impl, ITfThreadFocusSink,
    ITfThreadFocusSink_Impl, ITfThreadMgr,
};
use windows::Win32::UI::WindowsAndMessaging::{KillTimer, SetTimer};
use windows_core::{
    BOOL, BSTR, ComObject, GUID, HRESULT, HSTRING, IUnknown, IUnknownImpl, Interface, Ref, Result,
    implement, s,
};

use crate::CLSID_SWEET_IME;

/// Names the core to use, next to this DLL.
const CORE_POINTER: &str = "core.txt";
/// The core when there is no `core.txt` (development builds).
const DEV_CORE: &str = "sweet_tsf_core.dll";
/// Private `ActivateEx` flag for the core: keep the input mode, no badge.
const ACTIVATE_HOT_SWAP: u32 = 0x4000_0000;
/// Look for a new core at most this often.
const CHECK_INTERVAL: Duration = Duration::from_secs(2);
/// Let the focus change finish before switching.
const SWAP_DELAY_MS: u32 = 50;

type GetClassObject =
    unsafe extern "system" fn(*const GUID, *const GUID, *mut *mut c_void) -> HRESULT;
type Busy = unsafe extern "system" fn() -> BOOL;

#[derive(Clone)]
struct Core {
    name: String,
    get_class_object: GetClassObject,
    busy: Option<Busy>,
}

/// Cores loaded in this process, oldest first.
static CORES: Mutex<Vec<Core>> = Mutex::new(Vec::new());

pub(crate) fn latest_core_name() -> String {
    CORES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .last()
        .map(|c| c.name.clone())
        .unwrap_or_default()
}

/// The core `core.txt` names, or the development core.
fn wanted_core() -> String {
    let dir = crate::dll_path()
        .parent()
        .map(|d| d.to_path_buf())
        .unwrap_or_default();
    std::fs::read_to_string(dir.join(CORE_POINTER))
        .ok()
        .map(|s| s.trim().to_string())
        // A plain file name next to this DLL.
        .filter(|s| s.ends_with(".dll") && !s.contains(['\\', '/', ':']))
        .unwrap_or_else(|| DEV_CORE.to_string())
}

fn load_core(name: &str) -> Result<Core> {
    let mut cores = CORES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(c) = cores.iter().find(|c| c.name == name) {
        return Ok(c.clone());
    }
    let path = crate::dll_path()
        .parent()
        .map(|d| d.join(name))
        .ok_or(windows_core::Error::from(E_FAIL))?;
    unsafe {
        let module = LoadLibraryExW(
            &HSTRING::from(path.as_os_str()),
            None,
            LOAD_WITH_ALTERED_SEARCH_PATH,
        )?;
        let get = GetProcAddress(module, s!("DllGetClassObject"))
            .ok_or(windows_core::Error::from(E_FAIL))?;
        let busy = GetProcAddress(module, s!("SweetCoreBusy"));
        let core = Core {
            name: name.to_string(),
            get_class_object: std::mem::transmute::<_, GetClassObject>(get),
            busy: busy.map(|b| std::mem::transmute::<_, Busy>(b)),
        };
        cores.push(core.clone());
        Ok(core)
    }
}

/// A text service created by a core.
#[derive(Clone)]
struct Inner {
    core: Core,
    unk: IUnknown,
}

impl Inner {
    fn new(core: Core) -> Result<Self> {
        unsafe {
            let mut factory: *mut c_void = std::ptr::null_mut();
            (core.get_class_object)(&CLSID_SWEET_IME, &IClassFactory::IID, &mut factory).ok()?;
            let factory = IClassFactory::from_raw(factory);
            let unk: IUnknown = factory.CreateInstance(None)?;
            Ok(Self { core, unk })
        }
    }

    fn busy(&self) -> bool {
        self.core.busy.is_some_and(|f| unsafe { f() }.as_bool())
    }

    fn activate(&self, tm: &ITfThreadMgr, tid: u32, flags: u32) -> Result<()> {
        unsafe {
            self.unk
                .cast::<ITfTextInputProcessorEx>()?
                .ActivateEx(tm, tid, flags)
        }
    }

    fn deactivate(&self) {
        if let Ok(tip) = self.unk.cast::<ITfTextInputProcessor>() {
            unsafe {
                let _ = tip.Deactivate();
            }
        }
    }
}

#[derive(Default)]
struct State {
    inner: Option<Inner>,
    thread_mgr: Option<ITfThreadMgr>,
    tid: u32,
    flags: u32,
    focus_cookie: Option<u32>,
    last_check: Option<Instant>,
}

#[implement(
    ITfTextInputProcessorEx,
    ITfTextInputProcessor,
    ITfThreadFocusSink,
    ITfDisplayAttributeProvider,
    ITfFnConfigure,
    ITfFunction
)]
pub struct Shim {
    st: RefCell<State>,
}

impl Shim {
    pub fn new() -> Self {
        crate::lock_module(true);
        Self {
            st: RefCell::new(State::default()),
        }
    }
}

impl Drop for Shim {
    fn drop(&mut self) {
        crate::lock_module(false);
    }
}

impl Shim_Impl {
    /// The core's text service, created on first use.
    fn inner(&self) -> Result<IUnknown> {
        let mut st = self
            .st
            .try_borrow_mut()
            .map_err(|_| windows_core::Error::from(E_FAIL))?;
        if st.inner.is_none() {
            st.inner = Some(Inner::new(load_core(&wanted_core())?)?);
        }
        Ok(st.inner.as_ref().map(|i| i.unk.clone()).unwrap())
    }

    /// Switches to the core `core.txt` names, if it is new and nothing is
    /// being typed. Runs from the message loop, outside TSF callbacks.
    fn swap_if_updated(&self) {
        let _ = crate::guard(|| {
            let wanted = wanted_core();
            let (tm, tid, flags, old) = {
                let Ok(st) = self.st.try_borrow() else {
                    return Ok(());
                };
                let Some(tm) = st.thread_mgr.clone() else {
                    return Ok(());
                };
                match &st.inner {
                    Some(i) if i.core.name != wanted && !i.busy() => {}
                    _ => return Ok(()),
                }
                (tm, st.tid, st.flags, st.inner.clone())
            };
            // Keep the current core on failure; try again on a later focus.
            let Ok(new) = load_core(&wanted).and_then(Inner::new) else { return Ok(()) };
            // `st.inner` always names the text service being (de)activated:
            // TSF may call back into this shim meanwhile, and an empty slot
            // would make `inner()` create a second one.
            if let Some(old) = &old {
                old.deactivate();
            }
            self.st.borrow_mut().inner = Some(new.clone());
            if new.activate(&tm, tid, flags | ACTIVATE_HOT_SWAP).is_err() {
                new.deactivate();
                self.st.borrow_mut().inner = old.clone();
                if let Some(old) = &old {
                    let _ = old.activate(&tm, tid, flags | ACTIVATE_HOT_SWAP);
                }
            }
            Ok(())
        });
    }
}

thread_local! {
    /// Shims waiting for their swap timer, by timer id.
    static PENDING: RefCell<Vec<(usize, ComObject<Shim>)>> = const { RefCell::new(Vec::new()) };
}

unsafe extern "system" fn on_swap_timer(_hwnd: HWND, _msg: u32, id: usize, _time: u32) {
    unsafe {
        let _ = KillTimer(None, id);
    }
    // A panic must not unwind into the host application (that aborts it).
    let _ = std::panic::catch_unwind(|| {
        let shim = PENDING.with(|p| {
            let mut p = p.try_borrow_mut().ok()?;
            p.iter().position(|(t, _)| *t == id).map(|i| p.remove(i).1)
        });
        if let Some(shim) = shim {
            shim.swap_if_updated();
        }
    });
}

impl ITfTextInputProcessor_Impl for Shim_Impl {
    fn Activate(&self, ptim: Ref<ITfThreadMgr>, tid: u32) -> Result<()> {
        self.ActivateEx(ptim, tid, 0)
    }

    fn Deactivate(&self) -> Result<()> {
        crate::guard(|| {
            let (inner, tm, cookie) = {
                let mut st = self.st.borrow_mut();
                (
                    st.inner.take(),
                    st.thread_mgr.take(),
                    st.focus_cookie.take(),
                )
            };
            if let (Some(tm), Some(cookie)) = (tm, cookie) {
                if let Ok(src) = tm.cast::<ITfSource>() {
                    unsafe {
                        let _ = src.UnadviseSink(cookie);
                    }
                }
            }
            if let Some(inner) = inner {
                inner.deactivate();
            }
            Ok(())
        })
    }
}

impl ITfTextInputProcessorEx_Impl for Shim_Impl {
    fn ActivateEx(&self, ptim: Ref<ITfThreadMgr>, tid: u32, flags: u32) -> Result<()> {
        crate::guard(|| {
            let tm = ptim.ok()?.clone();
            let wanted = wanted_core();
            let reuse = self
                .st
                .borrow_mut()
                .inner
                .take()
                .filter(|i| i.core.name == wanted);
            let inner = match reuse {
                Some(i) => i,
                None => Inner::new(load_core(&wanted)?)?,
            };
            // In place before activating: TSF may call back into this shim
            // meanwhile (see `swap_if_updated`).
            self.st.borrow_mut().inner = Some(inner.clone());
            if let Err(e) = inner.activate(&tm, tid, flags) {
                self.st.borrow_mut().inner = None;
                return Err(e);
            }
            let sink: ITfThreadFocusSink = self.to_interface();
            let cookie = tm
                .cast::<ITfSource>()
                .and_then(|src| unsafe { src.AdviseSink(&ITfThreadFocusSink::IID, &sink) })
                .ok();
            let mut st = self.st.borrow_mut();
            st.inner = Some(inner);
            st.thread_mgr = Some(tm);
            st.tid = tid;
            st.flags = flags;
            st.focus_cookie = cookie;
            st.last_check = Some(Instant::now());
            Ok(())
        })
    }
}

impl ITfThreadFocusSink_Impl for Shim_Impl {
    fn OnSetThreadFocus(&self) -> Result<()> {
        let _ = crate::guard(|| {
            {
                let Ok(mut st) = self.st.try_borrow_mut() else {
                    return Ok(());
                };
                if st.last_check.is_some_and(|t| t.elapsed() < CHECK_INTERVAL) {
                    return Ok(());
                }
                st.last_check = Some(Instant::now());
                let current = st.inner.as_ref().map(|i| i.core.name.clone());
                if current.is_none() || current.as_deref() == Some(wanted_core().as_str()) {
                    return Ok(());
                }
            }
            let id = unsafe { SetTimer(None, 0, SWAP_DELAY_MS, Some(on_swap_timer)) };
            if id != 0 {
                let me: ComObject<Shim> = self.to_object();
                PENDING.with(|p| p.borrow_mut().push((id, me)));
            }
            Ok(())
        });
        Ok(())
    }

    fn OnKillThreadFocus(&self) -> Result<()> {
        Ok(())
    }
}

impl ITfDisplayAttributeProvider_Impl for Shim_Impl {
    fn EnumDisplayAttributeInfo(&self) -> Result<IEnumTfDisplayAttributeInfo> {
        crate::guard(|| unsafe {
            self.inner()?
                .cast::<ITfDisplayAttributeProvider>()?
                .EnumDisplayAttributeInfo()
        })
    }

    fn GetDisplayAttributeInfo(&self, guid: *const GUID) -> Result<ITfDisplayAttributeInfo> {
        crate::guard(|| unsafe {
            self.inner()?
                .cast::<ITfDisplayAttributeProvider>()?
                .GetDisplayAttributeInfo(guid)
        })
    }
}

/// "Options" in Windows language settings.
impl ITfFunction_Impl for Shim_Impl {
    fn GetDisplayName(&self) -> Result<BSTR> {
        crate::guard(|| unsafe { self.inner()?.cast::<ITfFunction>()?.GetDisplayName() })
    }
}

impl ITfFnConfigure_Impl for Shim_Impl {
    fn Show(&self, parent: HWND, langid: u16, profile: *const GUID) -> Result<()> {
        crate::guard(|| unsafe {
            self.inner()?
                .cast::<ITfFnConfigure>()?
                .Show(parent, langid, profile)
        })
    }
}
