//! Windows Terminal can route system keys to its control without a TSF event.
//! Handle only the configured Alt+backquote mode shortcut on that UI thread.

use std::cell::RefCell;
use std::rc::Rc;

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, HHOOK, MSG, PM_REMOVE, SetWindowsHookExW, UnhookWindowsHookEx, WH_GETMESSAGE,
    WM_KEYDOWN, WM_NULL, WM_SYSKEYDOWN,
};

type Handler = Rc<dyn Fn(u32, isize) -> bool>;

thread_local! {
    static HANDLER: RefCell<Option<Handler>> = const { RefCell::new(None) };
}

pub struct TerminalHotkey(HHOOK);

impl TerminalHotkey {
    pub fn install(f: impl Fn(u32, isize) -> bool + 'static) -> Option<Self> {
        let mut path = [0u16; 1024];
        let n = unsafe { GetModuleFileNameW(None, &mut path) } as usize;
        let path = String::from_utf16_lossy(&path[..n]);
        if !path.rsplit('\\').next()?.eq_ignore_ascii_case("WindowsTerminal.exe") {
            return None;
        }
        Self::install_on_thread(f)
    }

    fn install_on_thread(f: impl Fn(u32, isize) -> bool + 'static) -> Option<Self> {
        if HANDLER.with(|handler| handler.borrow().is_some()) {
            return None;
        }
        let hook = unsafe { SetWindowsHookExW(WH_GETMESSAGE, Some(message_hook), None, GetCurrentThreadId()) }.ok()?;
        HANDLER.with(|handler| *handler.borrow_mut() = Some(Rc::new(f)));
        Some(Self(hook))
    }
}

impl Drop for TerminalHotkey {
    fn drop(&mut self) {
        let _ = HANDLER.try_with(|handler| handler.borrow_mut().take());
        let _ = unsafe { UnhookWindowsHookEx(self.0) };
    }
}

unsafe extern "system" fn message_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && wparam.0 == PM_REMOVE.0 as usize && lparam.0 != 0 {
        let _ = crate::guard(|| {
            let msg = unsafe { &mut *(lparam.0 as *mut MSG) };
            let vk = msg.wParam.0 as u32;
            if !msg.hwnd.is_invalid()
                && matches!(msg.message, WM_KEYDOWN | WM_SYSKEYDOWN)
                && matches!(vk, 0xC0 | 0x19 | 0xF3 | 0xF4)
                && crate::keys::alt_down(msg.lParam.0)
            {
                let callback = HANDLER.with(|handler| handler.borrow().clone());
                if callback.is_some_and(|f| f(vk, msg.lParam.0)) {
                    msg.message = WM_NULL;
                }
            }
            Ok(())
        });
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, HWND_MESSAGE, PeekMessageW, PostMessageW, WM_CHAR,
    };

    #[test]
    fn hook_consumes_the_alt_shortcut_without_eating_at_signs() {
        let _hook = TerminalHotkey::install_on_thread(|_, _| true).unwrap();
        unsafe {
            let window = CreateWindowExW(
                Default::default(), windows_core::w!("STATIC"), windows_core::w!(""),
                Default::default(), 0, 0, 0, 0, Some(HWND_MESSAGE), None, None, None,
            ).unwrap();
            let next = |kind, key, flags| {
                PostMessageW(Some(window), kind, WPARAM(key), LPARAM(flags)).unwrap();
                let mut msg = MSG::default();
                assert!(PeekMessageW(&mut msg, Some(window), kind, kind, PM_REMOVE).as_bool());
                msg.message
            };
            let alt_backquote = next(WM_SYSKEYDOWN, 0xC0, 1 << 29);
            let plain_at = next(WM_CHAR, b'@' as usize, 0);
            let alt_a = next(WM_SYSKEYDOWN, 0x41, 1 << 29);
            DestroyWindow(window).unwrap();
            assert_eq!(alt_backquote, WM_NULL);
            assert_eq!(plain_at, WM_CHAR);
            assert_eq!(alt_a, WM_SYSKEYDOWN);
        }
    }
}
