use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, VIRTUAL_KEY,
    VK_CONTROL, VK_SHIFT, VK_MENU, VK_LWIN, VK_ESCAPE, VK_TAB, VK_LEFT, VK_RIGHT, VK_UP, VK_DOWN,
    VK_HOME, VK_END, VK_PRIOR, VK_NEXT, VK_F5, VK_F11, VK_F12, VK_A, VK_C, VK_D, VK_F, VK_H, VK_J, VK_L,
    VK_N, VK_R, VK_T, VK_V, VK_W,
};

#[derive(Debug, Clone, Copy)]
pub enum Key {
    Control,
    Shift,
    Alt,
    Meta,
    Escape,
    Tab,
    LeftArrow,
    RightArrow,
    UpArrow,
    DownArrow,
    Home,
    End,
    PageUp,
    PageDown,
    F5,
    F11,
    F12,
    Unicode(char),
}

fn key_to_vk(key: Key) -> Option<VIRTUAL_KEY> {
    match key {
        Key::Control => Some(VK_CONTROL),
        Key::Shift => Some(VK_SHIFT),
        Key::Alt => Some(VK_MENU),
        Key::Meta => Some(VK_LWIN),
        Key::Escape => Some(VK_ESCAPE),
        Key::Tab => Some(VK_TAB),
        Key::LeftArrow => Some(VK_LEFT),
        Key::RightArrow => Some(VK_RIGHT),
        Key::UpArrow => Some(VK_UP),
        Key::DownArrow => Some(VK_DOWN),
        Key::Home => Some(VK_HOME),
        Key::End => Some(VK_END),
        Key::PageUp => Some(VK_PRIOR),
        Key::PageDown => Some(VK_NEXT),
        Key::F5 => Some(VK_F5),
        Key::F11 => Some(VK_F11),
        Key::F12 => Some(VK_F12),
        Key::Unicode('a') | Key::Unicode('A') => Some(VK_A),
        Key::Unicode('c') | Key::Unicode('C') => Some(VK_C),
        Key::Unicode('d') | Key::Unicode('D') => Some(VK_D),
        Key::Unicode('f') | Key::Unicode('F') => Some(VK_F),
        Key::Unicode('h') | Key::Unicode('H') => Some(VK_H),
        Key::Unicode('j') | Key::Unicode('J') => Some(VK_J),
        Key::Unicode('l') | Key::Unicode('L') => Some(VK_L),
        Key::Unicode('n') | Key::Unicode('N') => Some(VK_N),
        Key::Unicode('r') | Key::Unicode('R') => Some(VK_R),
        Key::Unicode('t') | Key::Unicode('T') => Some(VK_T),
        Key::Unicode('v') | Key::Unicode('V') => Some(VK_V),
        Key::Unicode('w') | Key::Unicode('W') => Some(VK_W),
        Key::Unicode(ch) if ch.is_ascii_digit() => Some(VIRTUAL_KEY(ch as u16)),
        Key::Unicode(_) => None, // Use unicode flags instead
    }
}

pub fn send_key_press(key: Key) {
    if let Some(vk) = key_to_vk(key) {
        send_input(vk, 0, 0);
    } else if let Key::Unicode(ch) = key {
        let mut buf = [0; 2];
        for u in ch.encode_utf16(&mut buf) {
            send_input(VIRTUAL_KEY(0), *u, KEYEVENTF_UNICODE.0);
        }
    }
}

pub fn send_key_release(key: Key) {
    if let Some(vk) = key_to_vk(key) {
        send_input(vk, 0, KEYEVENTF_KEYUP.0);
    } else if let Key::Unicode(ch) = key {
        let mut buf = [0; 2];
        for u in ch.encode_utf16(&mut buf) {
            send_input(VIRTUAL_KEY(0), *u, KEYEVENTF_UNICODE.0 | KEYEVENTF_KEYUP.0);
        }
    }
}

pub fn send_key_click(key: Key) {
    send_key_press(key);
    send_key_release(key);
}

fn send_input(vk: VIRTUAL_KEY, scan_code: u16, flags: u32) {
    let ki = KEYBDINPUT {
        wVk: vk,
        wScan: scan_code,
        dwFlags: windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS(flags),
        time: 0,
        dwExtraInfo: 0,
    };

    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki },
    };

    unsafe {
        SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
    }
}
