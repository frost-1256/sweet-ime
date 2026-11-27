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

pub fn alt_down(lparam: isize) -> bool {
    // A host can forward WM_SYSKEYDOWN after its modifier state has changed.
    lparam & (1 << 29) != 0 || down(VK_MENU.0)
}

/// TSF can report a generic VK even though the keyboard message identifies the side.
pub fn modifier_vk(vk: u32, lparam: isize) -> Option<u16> {
    match vk {
        0x10 => Some(if (lparam >> 16) & 0xff == 0x36 {
            0xA1
        } else {
            0xA0
        }),
        0x11 => Some(if lparam & (1 << 24) != 0 { 0xA3 } else { 0xA2 }),
        0x12 => Some(if lparam & (1 << 24) != 0 { 0xA5 } else { 0xA4 }),
        0xA0..=0xA5 => Some(vk as u16),
        _ => None,
    }
}

pub fn modifier_alone(vk: u16) -> bool {
    [0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0x5B, 0x5C]
        .into_iter()
        .all(|other| other == vk || !down(other))
}

pub fn action_input(action: Action) -> Input {
    match action {
        Action::ImeToggle => Input::Toggle(Toggle::Toggle),
        Action::ImeOn => Input::Toggle(Toggle::On),
        Action::ImeOff => Input::Toggle(Toggle::Off),
        Action::Key(key) => Input::Key(key),
    }
}

/// Translates a key-down event. `composing`: whether a composition is in
/// progress (selects state-specific bindings). Returns `None` for keys the
/// IME does not handle.
pub fn translate(keymap: &Keymap, vk: u32, lparam: isize, composing: bool) -> Option<Input> {
    let chord = Chord { vk: vk as u16, ctrl: down(VK_CONTROL.0), shift: down(VK_SHIFT.0), alt: alt_down(lparam) };
    if let Some(action) = keymap.lookup(chord, composing) {
        return Some(action_input(action));
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
    let key = if chord.shift && c.is_ascii_alphabetic() {
        Key::ShiftedLetter(c)
    } else {
        Key::Char(c)
    };
    Some(Input::Key(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_modifier_events_preserve_the_physical_side() {
        assert_eq!(modifier_vk(0x10, 0x2A << 16), Some(0xA0));
        assert_eq!(modifier_vk(0x10, 0x36 << 16), Some(0xA1));
        assert_eq!(modifier_vk(0x11, 0), Some(0xA2));
        assert_eq!(modifier_vk(0x11, 1 << 24), Some(0xA3));
        assert_eq!(modifier_vk(0x12, 0), Some(0xA4));
        assert_eq!(modifier_vk(0x12, 1 << 24), Some(0xA5));
        assert_eq!(modifier_vk(0xA5, 0), Some(0xA5));
    }

    #[test]
    fn system_key_context_preserves_alt_on_backquote() {
        let map = Keymap::default();
        let alt_context = 1 << 29;
        for vk in [0xC0, 0x19, 0xF3, 0xF4] {
            assert!(matches!(translate(&map, vk, alt_context, false), Some(Input::Toggle(Toggle::Toggle))));
        }
    }
}
