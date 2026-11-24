//! Windows settings-window ownership and activation across processes.

use std::cell::Cell;
use std::io;
use std::time::{Duration, Instant};

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::{
    CloseHandle, HANDLE, HWND, LPARAM, LRESULT, WAIT_ABANDONED, WAIT_OBJECT_0, WAIT_TIMEOUT, WPARAM,
};
use windows::Win32::System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, EnumWindows, FLASHW_TRAY, FLASHWINFO, FlashWindowEx, GetPropW,
    GetWindowThreadProcessId, IsIconic, RegisterWindowMessageW, RemovePropW, SMTO_ABORTIFHUNG,
    SW_RESTORE, SendMessageTimeoutW, SetForegroundWindow, SetPropW, ShowWindow, WM_NCDESTROY,
};
use windows::core::{BOOL, PCWSTR, w};

use super::{Page, eframe, egui};

const MUTEX: PCWSTR = w!("Local\\SweetIME.Settings.5F541EB8-E274-42C3-82BF-6D80A89CCFB7");
const WINDOW_MARKER: PCWSTR = w!("SweetIME.Settings.5F541EB8-E274-42C3-82BF-6D80A89CCFB7");
const SUBCLASS_ID: usize = 1;

type AppError = Box<dyn std::error::Error + Send + Sync>;

pub(super) struct Instance {
    mutex: HANDLE,
    owned: bool,
}

impl Instance {
    /// The settings window keeps the mutex until its event loop has exited.
    /// A concurrent launch waits for the window to register or for its owner
    /// to exit, so it can never start a second event loop.
    pub(super) fn start(page: Option<Page>) -> Result<Option<Self>, AppError> {
        let mut instance = Self {
            mutex: unsafe { CreateMutexW(None, false, MUTEX)? },
            owned: false,
        };
        let message = activation_message()?;
        let page = page
            .and_then(|page| Page::ALL.iter().position(|&item| item == page))
            .map_or(0, |index| index + 1);
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            match unsafe { WaitForSingleObject(instance.mutex, 40) } {
                WAIT_OBJECT_0 | WAIT_ABANDONED => {
                    instance.owned = true;
                    return Ok(Some(instance));
                }
                WAIT_TIMEOUT => {}
                _ => return Err(windows::core::Error::from_thread().into()),
            }
            if activate_existing(message, page) || Instant::now() >= deadline {
                return Ok(None);
            }
        }
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        unsafe {
            if self.owned {
                let _ = ReleaseMutex(self.mutex);
            }
            let _ = CloseHandle(self.mutex);
        }
    }
}

fn activation_message() -> Result<u32, AppError> {
    match unsafe { RegisterWindowMessageW(WINDOW_MARKER) } {
        0 => Err(windows::core::Error::from_thread().into()),
        message => Ok(message),
    }
}

fn activate_existing(message: u32, page: usize) -> bool {
    unsafe extern "system" fn find(hwnd: HWND, result: LPARAM) -> BOOL {
        unsafe {
            if GetPropW(hwnd, WINDOW_MARKER).is_invalid() {
                return BOOL(1);
            }
            (result.0 as *mut HWND).write(hwnd);
            BOOL(0)
        }
    }

    let mut hwnd = HWND::default();
    unsafe {
        let _ = EnumWindows(Some(find), LPARAM((&mut hwnd as *mut HWND) as isize));
        if hwnd.is_invalid() {
            return false;
        }
        // A launcher started by user interaction can pass its foreground
        // permission to the process that owns the existing window.
        let mut pid = 0;
        if GetWindowThreadProcessId(hwnd, Some(&mut pid)) == 0 {
            return false;
        }
        let _ = AllowSetForegroundWindow(pid);
        let mut accepted = 0;
        SendMessageTimeoutW(
            hwnd,
            message,
            WPARAM(page),
            LPARAM(0),
            SMTO_ABORTIFHUNG,
            500,
            Some(&mut accepted),
        )
        .0 != 0
            && accepted != 0
    }
}

struct Request {
    message: u32,
    page: Cell<Option<Page>>,
    context: egui::Context,
}

pub(super) struct Activation {
    hwnd: HWND,
    request: Box<Request>,
}

impl Activation {
    pub(super) fn register(cc: &eframe::CreationContext<'_>) -> Result<Self, AppError> {
        let RawWindowHandle::Win32(handle) = cc.window_handle()?.as_raw() else {
            return Err(io::Error::other("settings window has no Win32 handle").into());
        };
        let hwnd = HWND(handle.hwnd.get() as *mut _);
        let request = Box::new(Request {
            message: activation_message()?,
            page: Cell::new(None),
            context: cc.egui_ctx.clone(),
        });
        unsafe {
            let data = (&*request as *const Request) as usize;
            if !SetWindowSubclass(hwnd, Some(window_proc), SUBCLASS_ID, data).as_bool() {
                return Err(
                    io::Error::other("could not register settings activation handler").into(),
                );
            }
            let activation = Self { hwnd, request };
            SetPropW(hwnd, WINDOW_MARKER, Some(HANDLE(1 as *mut _)))?;
            Ok(activation)
        }
    }

    pub(super) fn take_page(&self) -> Option<Page> {
        self.request.page.take()
    }
}

impl Drop for Activation {
    fn drop(&mut self) {
        unsafe {
            let _ = RemovePropW(self.hwnd, WINDOW_MARKER);
            let _ = RemoveWindowSubclass(self.hwnd, Some(window_proc), SUBCLASS_ID);
        }
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    unsafe {
        if message == WM_NCDESTROY {
            let _ = RemovePropW(hwnd, WINDOW_MARKER);
            let _ = RemoveWindowSubclass(hwnd, Some(window_proc), SUBCLASS_ID);
        } else {
            let request = &*(data as *const Request);
            if message == request.message {
                if let Some(&page) = wparam.0.checked_sub(1).and_then(|i| Page::ALL.get(i)) {
                    request.page.set(Some(page));
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        request.context.request_repaint();
                    }));
                }
                if IsIconic(hwnd).as_bool() {
                    let _ = ShowWindow(hwnd, SW_RESTORE);
                }
                if !SetForegroundWindow(hwnd).as_bool() {
                    let _ = FlashWindowEx(&FLASHWINFO {
                        cbSize: std::mem::size_of::<FLASHWINFO>() as u32,
                        hwnd,
                        dwFlags: FLASHW_TRAY,
                        uCount: 3,
                        dwTimeout: 0,
                    });
                }
                return LRESULT(1);
            }
        }
        DefSubclassProc(hwnd, message, wparam, lparam)
    }
}
