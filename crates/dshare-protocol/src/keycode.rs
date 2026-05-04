//! Keycode normalization.
//!
//! Wire `Message::KeyEvent.keycode` is a Linux evdev key code (u16, see
//! `<linux/input-event-codes.h>`). All other backends translate to/from
//! this canonical form.

/// Translate a Win32 virtual-key code to its evdev equivalent.
/// Returns `None` for unmapped keys (e.g. media/browser keys, IME).
pub fn vk_to_evdev(vk: u32) -> Option<u16> {
    Some(match vk {
        // Letters (VK_A=0x41 .. VK_Z=0x5A)
        0x41 => 30,  // A → KEY_A
        0x42 => 48,  // B → KEY_B
        0x43 => 46,  // C → KEY_C
        0x44 => 32,  // D → KEY_D
        0x45 => 18,  // E → KEY_E
        0x46 => 33,  // F → KEY_F
        0x47 => 34,  // G → KEY_G
        0x48 => 35,  // H → KEY_H
        0x49 => 23,  // I → KEY_I
        0x4A => 36,  // J → KEY_J
        0x4B => 37,  // K → KEY_K
        0x4C => 38,  // L → KEY_L
        0x4D => 50,  // M → KEY_M
        0x4E => 49,  // N → KEY_N
        0x4F => 24,  // O → KEY_O
        0x50 => 25,  // P → KEY_P
        0x51 => 16,  // Q → KEY_Q
        0x52 => 19,  // R → KEY_R
        0x53 => 31,  // S → KEY_S
        0x54 => 20,  // T → KEY_T
        0x55 => 22,  // U → KEY_U
        0x56 => 47,  // V → KEY_V
        0x57 => 17,  // W → KEY_W
        0x58 => 45,  // X → KEY_X
        0x59 => 21,  // Y → KEY_Y
        0x5A => 44,  // Z → KEY_Z

        // Top-row digits
        0x30 => 11,  // 0 → KEY_0
        0x31 => 2,   // 1 → KEY_1
        0x32 => 3,
        0x33 => 4,
        0x34 => 5,
        0x35 => 6,
        0x36 => 7,
        0x37 => 8,
        0x38 => 9,
        0x39 => 10,

        // Whitespace / control
        0x0D => 28,  // VK_RETURN → KEY_ENTER
        0x1B => 1,   // VK_ESCAPE → KEY_ESC
        0x08 => 14,  // VK_BACK → KEY_BACKSPACE
        0x09 => 15,  // VK_TAB → KEY_TAB
        0x20 => 57,  // VK_SPACE → KEY_SPACE

        // Arrows
        0x25 => 105, // VK_LEFT → KEY_LEFT
        0x26 => 103, // VK_UP → KEY_UP
        0x27 => 106, // VK_RIGHT → KEY_RIGHT
        0x28 => 108, // VK_DOWN → KEY_DOWN

        // Modifiers (left/right specific)
        0xA0 => 42,  // VK_LSHIFT → KEY_LEFTSHIFT
        0xA1 => 54,  // VK_RSHIFT → KEY_RIGHTSHIFT
        0xA2 => 29,  // VK_LCONTROL → KEY_LEFTCTRL
        0xA3 => 97,  // VK_RCONTROL → KEY_RIGHTCTRL
        0xA4 => 56,  // VK_LMENU → KEY_LEFTALT
        0xA5 => 100, // VK_RMENU → KEY_RIGHTALT
        0x5B => 125, // VK_LWIN → KEY_LEFTMETA
        0x5C => 126, // VK_RWIN → KEY_RIGHTMETA

        // Function keys
        0x70 => 59,  // F1
        0x71 => 60,
        0x72 => 61,
        0x73 => 62,
        0x74 => 63,
        0x75 => 64,
        0x76 => 65,
        0x77 => 66,
        0x78 => 67,
        0x79 => 68,
        0x7A => 87,  // F11
        0x7B => 88,  // F12

        // Editing block
        0x2D => 110, // VK_INSERT → KEY_INSERT
        0x2E => 111, // VK_DELETE → KEY_DELETE
        0x24 => 102, // VK_HOME → KEY_HOME
        0x23 => 107, // VK_END → KEY_END
        0x21 => 104, // VK_PRIOR → KEY_PAGEUP
        0x22 => 109, // VK_NEXT → KEY_PAGEDOWN

        // Punctuation (US layout)
        0xBA => 39,  // VK_OEM_1 (;:) → KEY_SEMICOLON
        0xBB => 13,  // VK_OEM_PLUS (=+) → KEY_EQUAL
        0xBC => 51,  // VK_OEM_COMMA → KEY_COMMA
        0xBD => 12,  // VK_OEM_MINUS → KEY_MINUS
        0xBE => 52,  // VK_OEM_PERIOD → KEY_DOT
        0xBF => 53,  // VK_OEM_2 (/?) → KEY_SLASH
        0xC0 => 41,  // VK_OEM_3 (`~) → KEY_GRAVE
        0xDB => 26,  // VK_OEM_4 ([{) → KEY_LEFTBRACE
        0xDC => 43,  // VK_OEM_5 (\|) → KEY_BACKSLASH
        0xDD => 27,  // VK_OEM_6 (]}) → KEY_RIGHTBRACE
        0xDE => 40,  // VK_OEM_7 ('") → KEY_APOSTROPHE

        // Locks
        0x14 => 58,  // VK_CAPITAL → KEY_CAPSLOCK
        0x90 => 69,  // VK_NUMLOCK → KEY_NUMLOCK
        0x91 => 70,  // VK_SCROLL → KEY_SCROLLLOCK

        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_map_correctly() {
        assert_eq!(vk_to_evdev(0x48), Some(35)); // H → KEY_H
        assert_eq!(vk_to_evdev(0x49), Some(23)); // I → KEY_I
    }

    #[test]
    fn enter_and_escape() {
        assert_eq!(vk_to_evdev(0x0D), Some(28));
        assert_eq!(vk_to_evdev(0x1B), Some(1));
    }

    #[test]
    fn arrows() {
        assert_eq!(vk_to_evdev(0x25), Some(105));
        assert_eq!(vk_to_evdev(0x26), Some(103));
        assert_eq!(vk_to_evdev(0x27), Some(106));
        assert_eq!(vk_to_evdev(0x28), Some(108));
    }

    #[test]
    fn unknown_returns_none() {
        assert_eq!(vk_to_evdev(0xFFFF), None);
    }
}
