//! X keysym -> Windows virtual-key code, so the shared key map applies.

/// The virtual-key code for `sym`, or `None` for keys the key map cannot
/// name (they can still type their character).
pub fn to_vk(sym: u32) -> Option<u16> {
    let vk = match sym {
        0x20 => 0x20,
        // Letters (either case) and digits.
        0x61..=0x7a => (sym - 0x20) as u16,
        0x41..=0x5a | 0x30..=0x39 => sym as u16,
        // Punctuation keys (US layout positions, as VK_OEM_*).
        0x3b | 0x3a => 0xBA,
        0x3d | 0x2b => 0xBB,
        0x2c | 0x3c => 0xBC,
        0x2d | 0x5f => 0xBD,
        0x2e | 0x3e => 0xBE,
        0x2f | 0x3f => 0xBF,
        0x60 | 0x7e => 0xC0,
        0x5b | 0x7b => 0xDB,
        0x5c | 0x7c => 0xDC,
        0x5d | 0x7d => 0xDD,
        0x27 | 0x22 => 0xDE,
        0xff08 => 0x08,                                    // BackSpace
        0xff09 | 0xfe20 => 0x09,                           // Tab, ISO_Left_Tab
        0xff0d | 0xff8d => 0x0D,                           // Return, KP_Enter
        0xff1b => 0x1B,                                    // Escape
        0xffff | 0xff9f => 0x2E,                           // Delete, KP_Delete
        0xff50 | 0xff95 => 0x24,                           // Home
        0xff51 | 0xff96 => 0x25,                           // Left
        0xff52 | 0xff97 => 0x26,                           // Up
        0xff53 | 0xff98 => 0x27,                           // Right
        0xff54 | 0xff99 => 0x28,                           // Down
        0xff55 | 0xff9a => 0x21,                           // Prior
        0xff56 | 0xff9b => 0x22,                           // Next
        0xff57 | 0xff9c => 0x23,                           // End
        0xffbe..=0xffc9 => (0x70 + (sym - 0xffbe)) as u16, // F1..F12
        0xffe1 => 0xA0,                                    // Shift_L
        0xffe2 => 0xA1,                                    // Shift_R
        0xffe3 => 0xA2,                                    // Control_L
        0xffe4 => 0xA3,                                    // Control_R
        0xffe9 => 0xA4,                                    // Alt_L
        0xffea | 0xfe03 => 0xA5,                           // Alt_R, ISO_Level3_Shift (AltGr)
        // Japanese keyboard keys.
        0xff21 | 0xff28 | 0xff29 | 0xff2a => 0x19, // Kanji, Zenkaku, Hankaku, Zenkaku_Hankaku
        0xff22 => 0x1D,                            // Muhenkan
        0xff23 => 0x1C,                            // Henkan_Mode
        0xff25..=0xff27 => 0xF2,                   // Hiragana, Katakana, Hiragana_Katakana
        0xff2f | 0xff30 => 0xF0,                   // Eisu_Shift, Eisu_toggle
        _ => return None,
    };
    Some(vk)
}

#[cfg(test)]
mod tests {
    use super::to_vk;

    #[test]
    fn maps_common_keys() {
        assert_eq!(to_vk('a' as u32), Some(0x41));
        assert_eq!(to_vk('A' as u32), Some(0x41));
        assert_eq!(to_vk('7' as u32), Some(0x37));
        assert_eq!(to_vk(0xff0d), Some(0x0D));
        assert_eq!(to_vk(0xffc5), Some(0x77)); // F8
        assert_eq!(to_vk(0xff2a), Some(0x19));
        assert_eq!(to_vk(0xff51), Some(0x25));
        assert_eq!(to_vk(0x1234), None);
    }
}
