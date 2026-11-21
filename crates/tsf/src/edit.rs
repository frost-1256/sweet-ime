//! Closure-backed `ITfEditSession`.

use windows::Win32::UI::TextServices::{
    ITfContext, ITfEditSession, ITfEditSession_Impl, TF_ES_READWRITE, TF_ES_SYNC,
};
use windows_core::{Result, implement};

type EditFn = Box<dyn Fn(u32) -> Result<()>>;

#[implement(ITfEditSession)]
struct EditSession {
    f: EditFn,
}

impl ITfEditSession_Impl for EditSession_Impl {
    fn DoEditSession(&self, ec: u32) -> Result<()> {
        crate::guard(|| (self.f)(ec))
    }
}

/// Runs `f` inside a synchronous read/write edit session.
pub fn request<F>(ctx: &ITfContext, tid: u32, f: F) -> Result<()>
where
    F: Fn(u32) -> Result<()> + 'static,
{
    let session: ITfEditSession = EditSession { f: Box::new(f) }.into();
    unsafe {
        let hr = ctx.RequestEditSession(tid, &session, TF_ES_SYNC | TF_ES_READWRITE)?;
        hr.ok()
    }
}
