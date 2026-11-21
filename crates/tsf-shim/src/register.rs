//! COM server and TSF profile registration.

use crate::{CLSID_SWEET_IME, DISPLAY_NAME, GUID_PROFILE, LANGID_JAPANESE, dll_path};
use windows::Win32::Foundation::{ERROR_SUCCESS, WIN32_ERROR};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
    RegCreateKeyExW, RegDeleteTreeW, RegSetValueExW,
};
use windows::Win32::UI::Input::KeyboardAndMouse::HKL;
use windows::Win32::UI::TextServices::{
    CLSID_TF_CategoryMgr, CLSID_TF_InputProcessorProfiles, GUID_TFCAT_DISPLAYATTRIBUTEPROVIDER,
    GUID_TFCAT_TIP_KEYBOARD, GUID_TFCAT_TIPCAP_COMLESS, GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT,
    GUID_TFCAT_TIPCAP_INPUTMODECOMPARTMENT, GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT, ITfCategoryMgr,
    ITfInputProcessorProfileMgr,
};
use windows_core::{GUID, HSTRING, PCWSTR, Result};

const CATEGORIES: &[GUID] = &[
    GUID_TFCAT_TIP_KEYBOARD,
    GUID_TFCAT_DISPLAYATTRIBUTEPROVIDER,
    GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT,
    GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT,
    GUID_TFCAT_TIPCAP_COMLESS,
    GUID_TFCAT_TIPCAP_INPUTMODECOMPARTMENT,
];

fn clsid_key() -> String {
    format!("SOFTWARE\\Classes\\CLSID\\{{{:?}}}", CLSID_SWEET_IME)
}

fn check(e: WIN32_ERROR) -> Result<()> {
    if e == ERROR_SUCCESS { Ok(()) } else { Err(e.to_hresult().into()) }
}

fn set_string(key: HKEY, name: Option<&str>, value: &str) -> Result<()> {
    let wide: Vec<u16> = value.encode_utf16().chain(Some(0)).collect();
    let bytes = unsafe { std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2) };
    let name_h = name.map(HSTRING::from);
    let name_p = name_h.as_ref().map(|h| PCWSTR(h.as_ptr())).unwrap_or(PCWSTR::null());
    check(unsafe { RegSetValueExW(key, name_p, None, REG_SZ, Some(bytes)) })
}

fn create_key(path: &str) -> Result<HKEY> {
    let mut key = HKEY::default();
    check(unsafe {
        RegCreateKeyExW(
            HKEY_LOCAL_MACHINE,
            &HSTRING::from(path),
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut key,
            None,
        )
    })?;
    Ok(key)
}

fn register_server() -> Result<()> {
    let base = clsid_key();
    let key = create_key(&base)?;
    let r = set_string(key, None, DISPLAY_NAME);
    unsafe { let _ = RegCloseKey(key); }
    r?;
    let key = create_key(&format!("{base}\\InprocServer32"))?;
    let path = dll_path();
    let r = set_string(key, None, &path.to_string_lossy())
        .and_then(|_| set_string(key, Some("ThreadingModel"), "Apartment"));
    unsafe { let _ = RegCloseKey(key); }
    r
}

pub fn register() -> Result<()> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    register_server()?;
    let path: Vec<u16> = dll_path().to_string_lossy().encode_utf16().collect();
    let name: Vec<u16> = DISPLAY_NAME.encode_utf16().collect();
    unsafe {
        let profiles: ITfInputProcessorProfileMgr =
            CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER)?;
        profiles.RegisterProfile(
            &CLSID_SWEET_IME,
            LANGID_JAPANESE,
            &GUID_PROFILE,
            &name,
            &path,
            0,
            HKL::default(),
            0,
            true,
            0,
        )?;
        let cats: ITfCategoryMgr =
            CoCreateInstance(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER)?;
        for cat in CATEGORIES {
            cats.RegisterCategory(&CLSID_SWEET_IME, cat, &CLSID_SWEET_IME)?;
        }
    }
    Ok(())
}

pub fn unregister() -> Result<()> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        if let Ok(cats) = CoCreateInstance::<_, ITfCategoryMgr>(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER) {
            for cat in CATEGORIES {
                let _ = cats.UnregisterCategory(&CLSID_SWEET_IME, cat, &CLSID_SWEET_IME);
            }
        }
        if let Ok(profiles) = CoCreateInstance::<_, ITfInputProcessorProfileMgr>(
            &CLSID_TF_InputProcessorProfiles,
            None,
            CLSCTX_INPROC_SERVER,
        ) {
            let _ = profiles.UnregisterProfile(&CLSID_SWEET_IME, LANGID_JAPANESE, &GUID_PROFILE, 0);
        }
        let _ = RegDeleteTreeW(HKEY_LOCAL_MACHINE, &HSTRING::from(clsid_key()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn clsid_key_format() {
        assert_eq!(
            super::clsid_key(),
            r"SOFTWARE\Classes\CLSID\{7E1D3C2A-5B8F-4C1E-9A3D-2F6B8C4E1A01}"
        );
    }
}
