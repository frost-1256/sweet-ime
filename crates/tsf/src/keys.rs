//! Virtual-key -> engine key translation.

use sweet_engine::Key;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, GetKeyboardState, ToUnicode, VK_BACK, VK_CONTROL, VK_DELETE, VK_DOWN, VK_END,
    VK_ESCAPE, VK_F6, VK_F7, VK_F8, VK_F9, VK_F10, VK_HOME, VK_LEFT, VK_MENU, VK_NEXT, VK_PRIOR,
    VK_RETURN, VK_RIGHT, VK_SHIFT, VK_SPACE, VK_TAB, VK_UP,
};

/// Keys that toggle the IME on/off.
pub enum Toggle {
    Toggle,
    On,
    Off,
}

pub fn toggle_key(vk: u32) -> Option<Toggle> {
    match vk {
        0x19 /* VK_KANJI */ | 0xF3 /* VK_OEM_AUTO */ | 0xF4 /* VK_OEM_ENLW */ => Some(Toggle::Toggle),
        0x16 /* VK_IME_ON */ | 0x1C /* VK_CONVERT */ => Some(Toggle::On),
        0x1A /* VK_IME_OFF */ | 0x1D /* VK_NONCONVERT */ => Some(Toggle::Off),
        0xF2 /* VK_OEM_COPY: ひらがな/カタカナ */ => Some(Toggle::On),
        0xF0 /* VK_OEM_ATTN: 英数 (Caps) */ => Some(Toggle::Off),
        _ => None,
    }
}

fn down(vk: u16) -> bool {
    unsafe { (GetKeyState(vk as i32) as u16 & 0x8000) != 0 }
}

/// Translates a key-down event. Returns `None` for keys the IME never handles
/// (modifier chords, function keys we don't use, ...).
pub fn translate(vk: u32, lparam: isize) -> Option<Key> {
    let ctrl = down(VK_CONTROL.0);
    let alt = down(VK_MENU.0);
    let shift = down(VK_SHIFT.0);
    if ctrl || alt {
        return None;
    }
    let vk16 = vk as u16;
    let key = match vk16 {
        v if v == VK_SPACE.0 => Key::Space,
        v if v == VK_RETURN.0 => Key::Enter,
        v if v == VK_BACK.0 => Key::Backspace,
        v if v == VK_DELETE.0 => Key::Delete,
        v if v == VK_ESCAPE.0 => Key::Escape,
        v if v == VK_TAB.0 => Key::Tab,
        v if v == VK_LEFT.0 => if shift { Key::ShrinkSegment } else { Key::Left },
        v if v == VK_RIGHT.0 => if shift { Key::ExtendSegment } else { Key::Right },
        v if v == VK_UP.0 => Key::Up,
        v if v == VK_DOWN.0 => Key::Down,
        v if v == VK_HOME.0 => Key::Home,
        v if v == VK_END.0 => Key::End,
        v if v == VK_PRIOR.0 => Key::PageUp,
        v if v == VK_NEXT.0 => Key::PageDown,
        v if v == VK_F6.0 => Key::ToHiragana,
        v if v == VK_F7.0 => Key::ToKatakana,
        v if v == VK_F8.0 => Key::ToHalfKatakana,
        v if v == VK_F9.0 => Key::ToFullAscii,
        v if v == VK_F10.0 => Key::ToHalfAscii,
        _ => {
            let mut state = [0u8; 256];
            unsafe { GetKeyboardState(&mut state).ok()? };
            let scan = ((lparam >> 16) & 0xff) as u32;
            let mut buf = [0u16; 4];
            // Flag 0x4: do not change the keyboard (dead key) state.
            let n = unsafe { ToUnicode(vk, scan, Some(&state), &mut buf, 0x4) };
            if n != 1 {
                return None;
            }
            let c = char::from_u32(buf[0] as u32)?;
            if c.is_control() || !c.is_ascii() {
                return None;
            }
            Key::Char(c)
        }
    };
    Some(key)
}
