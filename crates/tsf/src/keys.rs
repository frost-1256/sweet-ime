//! Virtual-key -> engine key translation through the user's key map.

use sweet_engine::Key;
use sweet_engine::keymap::{Action, Chord, Keymap};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, GetKeyboardState, ToUnicode, VK_CONTROL, VK_MENU, VK_SHIFT,
};

/// Keys that toggle the IME on/off.
#[derive(Clone, Copy)]
pub enum Toggle {
    Toggle,
    On,
    Off,
}

pub enum Input {
    Toggle(Toggle),
    Key(Key),
}

fn down(vk: u16) -> bool {
    unsafe { (GetKeyState(vk as i32) as u16 & 0x8000) != 0 }
}

/// Translates a key-down event. `composing`: whether a composition is in
/// progress (selects state-specific bindings). Returns `None` for keys the
/// IME does not handle.
pub fn translate(keymap: &Keymap, vk: u32, lparam: isize, composing: bool) -> Option<Input> {
    let chord = Chord { vk: vk as u16, ctrl: down(VK_CONTROL.0), shift: down(VK_SHIFT.0), alt: down(VK_MENU.0) };
    if let Some(action) = keymap.lookup(chord, composing) {
        return Some(match action {
            Action::ImeToggle => Input::Toggle(Toggle::Toggle),
            Action::ImeOn => Input::Toggle(Toggle::On),
            Action::ImeOff => Input::Toggle(Toggle::Off),
            Action::Key(k) => Input::Key(k),
        });
    }
    if chord.ctrl || chord.alt {
        return None;
    }
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
    if c.is_control() || !c.is_ascii() || c == ' ' {
        return None;
    }
    Some(Input::Key(Key::Char(c)))
}
