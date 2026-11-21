//! Display attributes (underlines) for the composition string.

use crate::{GUID_ATTR_CONVERTED, GUID_ATTR_FOCUSED, GUID_ATTR_INPUT};
use std::cell::Cell;
use windows::Win32::Foundation::{E_INVALIDARG, S_FALSE};
use windows::Win32::UI::TextServices::{
    IEnumTfDisplayAttributeInfo, IEnumTfDisplayAttributeInfo_Impl, ITfDisplayAttributeInfo,
    ITfDisplayAttributeInfo_Impl, TF_ATTR_INPUT, TF_ATTR_TARGET_CONVERTED, TF_ATTR_CONVERTED,
    TF_DISPLAYATTRIBUTE, TF_LS_DOT, TF_LS_SOLID,
};
use windows_core::{BOOL, BSTR, GUID, Result, implement};

pub const ALL: [GUID; 3] = [GUID_ATTR_INPUT, GUID_ATTR_CONVERTED, GUID_ATTR_FOCUSED];

fn attribute(guid: &GUID) -> Option<(TF_DISPLAYATTRIBUTE, &'static str)> {
    let mut a = TF_DISPLAYATTRIBUTE::default();
    let desc = if *guid == GUID_ATTR_INPUT {
        a.lsStyle = TF_LS_DOT;
        a.bAttr = TF_ATTR_INPUT;
        "Sweet IME input"
    } else if *guid == GUID_ATTR_CONVERTED {
        a.lsStyle = TF_LS_SOLID;
        a.bAttr = TF_ATTR_CONVERTED;
        "Sweet IME converted"
    } else if *guid == GUID_ATTR_FOCUSED {
        a.lsStyle = TF_LS_SOLID;
        a.fBoldLine = BOOL(1);
        a.bAttr = TF_ATTR_TARGET_CONVERTED;
        "Sweet IME focused"
    } else {
        return None;
    };
    Some((a, desc))
}

#[implement(ITfDisplayAttributeInfo)]
pub struct AttributeInfo {
    guid: GUID,
}

impl ITfDisplayAttributeInfo_Impl for AttributeInfo_Impl {
    fn GetGUID(&self) -> Result<GUID> {
        Ok(self.guid)
    }
    fn GetDescription(&self) -> Result<BSTR> {
        Ok(BSTR::from(attribute(&self.guid).map(|a| a.1).unwrap_or("")))
    }
    fn GetAttributeInfo(&self, pda: *mut TF_DISPLAYATTRIBUTE) -> Result<()> {
        if pda.is_null() {
            return Err(E_INVALIDARG.into());
        }
        let (a, _) = attribute(&self.guid).ok_or(windows_core::Error::from(E_INVALIDARG))?;
        unsafe { *pda = a };
        Ok(())
    }
    fn SetAttributeInfo(&self, _pda: *const TF_DISPLAYATTRIBUTE) -> Result<()> {
        Ok(())
    }
    fn Reset(&self) -> Result<()> {
        Ok(())
    }
}

pub fn info(guid: &GUID) -> Result<ITfDisplayAttributeInfo> {
    if attribute(guid).is_none() {
        return Err(E_INVALIDARG.into());
    }
    Ok(AttributeInfo { guid: *guid }.into())
}

#[implement(IEnumTfDisplayAttributeInfo)]
pub struct EnumInfo {
    pos: Cell<usize>,
}

impl EnumInfo {
    pub fn new() -> IEnumTfDisplayAttributeInfo {
        EnumInfo { pos: Cell::new(0) }.into()
    }
}

impl IEnumTfDisplayAttributeInfo_Impl for EnumInfo_Impl {
    fn Clone(&self) -> Result<IEnumTfDisplayAttributeInfo> {
        Ok(EnumInfo { pos: Cell::new(self.pos.get()) }.into())
    }
    fn Next(&self, count: u32, out: *mut Option<ITfDisplayAttributeInfo>, fetched: *mut u32) -> Result<()> {
        let mut n = 0u32;
        while n < count && self.pos.get() < ALL.len() {
            unsafe { out.add(n as usize).write(Some(info(&ALL[self.pos.get()])?)) };
            self.pos.set(self.pos.get() + 1);
            n += 1;
        }
        if !fetched.is_null() {
            unsafe { *fetched = n };
        }
        if n == count { Ok(()) } else { Err(S_FALSE.into()) }
    }
    fn Reset(&self) -> Result<()> {
        self.pos.set(0);
        Ok(())
    }
    fn Skip(&self, count: u32) -> Result<()> {
        let p = self.pos.get() + count as usize;
        self.pos.set(p.min(ALL.len()));
        if p <= ALL.len() { Ok(()) } else { Err(S_FALSE.into()) }
    }
}
