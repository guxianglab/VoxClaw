use std::mem::size_of;
use std::sync::mpsc::Sender;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
use std::thread;
use std::time::Duration;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{VK_MENU, VK_RMENU};
use windows::Win32::UI::Input::{
    GetRawInputData, RegisterRawInputDevices, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER,
    RIDEV_INPUTSINK, RID_INPUT, RIM_TYPEKEYBOARD, RIM_TYPEMOUSE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, PostQuitMessage,
    RegisterClassW, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, HWND_MESSAGE, MSG, WNDCLASSW,
    WM_DESTROY, WM_INPUT, GWLP_USERDATA,
};

// Raw Input constants not exported by the windows crate
const RI_KEY_BREAK: u16 = 1; // Key was released
const RI_KEY_E0: u16 = 2; // Extended key (e.g. right Alt vs left Alt)
const RI_MOUSE_MIDDLE_BUTTON_DOWN: u16 = 0x0010;
const RI_MOUSE_MIDDLE_BUTTON_UP: u16 = 0x0020;

const HOLD_THRESHOLD_MS: u64 = 350;

#[derive(Debug, Clone)]
pub enum InputEvent {
    Click,
    StartSkill,
    StopSkill,
    DictationFinalizeWindowElapsed {
        session_id: u64,
    },
    DictationAsrFinished {
        session_id: u64,
        result: Result<String, String>,
    },
}

pub struct InputListener {
    pub enable_mouse: Arc<AtomicBool>,
    pub enable_alt: Arc<AtomicBool>,
}

impl InputListener {
    pub fn new() -> Self {
        Self {
            enable_mouse: Arc::new(AtomicBool::new(true)),
            enable_alt: Arc::new(AtomicBool::new(true)),
        }
    }

    pub fn start(&self, tx: Sender<InputEvent>) {
        let enable_mouse = self.enable_mouse.clone();
        let enable_alt = self.enable_alt.clone();

        thread::spawn(move || {
            let middle_trigger = HoldTrigger::new();
            let alt_trigger = HoldTrigger::new();

            unsafe {
                let class_name = windows::core::w!("SonicClawRawInputClass");
                let wc = WNDCLASSW {
                    style: CS_HREDRAW | CS_VREDRAW,
                    lpfnWndProc: Some(raw_input_wndproc),
                    lpszClassName: class_name,
                    ..Default::default()
                };

                if RegisterClassW(&wc) == 0 {
                    eprintln!("[InputListener] Failed to register window class");
                    return;
                }

                let hwnd = match CreateWindowExW(
                    Default::default(),
                    class_name,
                    windows::core::w!(""),
                    Default::default(),
                    CW_USEDEFAULT,
                    CW_USEDEFAULT,
                    CW_USEDEFAULT,
                    CW_USEDEFAULT,
                    HWND_MESSAGE, // message-only window, invisible
                    None,
                    None,
                    None,
                ) {
                    Ok(h) => h,
                    Err(e) => {
                        eprintln!("[InputListener] Failed to create message-only window: {e}");
                        return;
                    }
                };

                // Store state pointer in GWLP_USERDATA so wndproc can access it
                let state = Box::new(ListenerState {
                    tx,
                    enable_mouse,
                    enable_alt,
                    middle_trigger,
                    alt_trigger,
                });
                windows::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW(
                    hwnd,
                    GWLP_USERDATA,
                    Box::into_raw(state) as isize,
                );

                // Register raw input for keyboard and mouse with RIDEV_INPUTSINK
                // so we receive WM_INPUT even when our window is not in the foreground.
                let devices = [
                    RAWINPUTDEVICE {
                        usUsagePage: 0x01, // HID_USAGE_PAGE_GENERIC
                        usUsage: 0x02,     // HID_USAGE_GENERIC_MOUSE
                        dwFlags: RIDEV_INPUTSINK,
                        hwndTarget: hwnd,
                    },
                    RAWINPUTDEVICE {
                        usUsagePage: 0x01, // HID_USAGE_PAGE_GENERIC
                        usUsage: 0x06,     // HID_USAGE_GENERIC_KEYBOARD
                        dwFlags: RIDEV_INPUTSINK,
                        hwndTarget: hwnd,
                    },
                ];

                if let Err(e) = RegisterRawInputDevices(&devices, size_of::<RAWINPUTDEVICE>() as u32) {
                    eprintln!("[InputListener] Failed to register raw input devices: {e}");
                    return;
                }

                println!("[InputListener] Raw Input listener started (no hooks, no latency)");

                // Message loop
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, HWND::default(), 0, 0).into() {
                    let _ = DispatchMessageW(&msg);
                }
            }
        });
    }
}

/// State shared between the listener thread and the window procedure.
struct ListenerState {
    tx: Sender<InputEvent>,
    enable_mouse: Arc<AtomicBool>,
    enable_alt: Arc<AtomicBool>,
    middle_trigger: HoldTrigger,
    alt_trigger: HoldTrigger,
}

unsafe extern "system" fn raw_input_wndproc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_INPUT => {
            let ptr = windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(
                window,
                GWLP_USERDATA,
            ) as *mut ListenerState;

            if !ptr.is_null() {
                let state = &*ptr;
                handle_raw_input(state, lparam);
            }

            DefWindowProcW(window, message, wparam, lparam)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(window, message, wparam, lparam),
    }
}

unsafe fn handle_raw_input(state: &ListenerState, lparam: LPARAM) {
    let hraw = HRAWINPUT(lparam.0 as *mut std::ffi::c_void);

    // First call: get required buffer size
    let mut dw_size = 0u32;
    let _ = GetRawInputData(
        hraw,
        RID_INPUT,
        None,
        &mut dw_size,
        size_of::<RAWINPUTHEADER>() as u32,
    );

    if dw_size == 0 {
        return;
    }

    // Second call: read the actual data
    let mut raw_buf: Vec<u8> = vec![0; dw_size as usize];
    let bytes_copied = GetRawInputData(
        hraw,
        RID_INPUT,
        Some(raw_buf.as_mut_ptr() as *mut std::ffi::c_void),
        &mut dw_size,
        size_of::<RAWINPUTHEADER>() as u32,
    );

    if bytes_copied != dw_size {
        return;
    }

    let raw = &*(raw_buf.as_ptr() as *const RAWINPUT);

    if raw.header.dwType == RIM_TYPEMOUSE.0 {
        let mouse = raw.data.mouse;
        let flags = mouse.Anonymous.Anonymous.usButtonFlags;

        if (flags & RI_MOUSE_MIDDLE_BUTTON_DOWN) != 0 {
            if state.enable_mouse.load(Ordering::Relaxed) {
                state.middle_trigger.on_press(&state.tx);
            }
        } else if (flags & RI_MOUSE_MIDDLE_BUTTON_UP) != 0 {
            if state.enable_mouse.load(Ordering::Relaxed) {
                state.middle_trigger.on_release(&state.tx);
            }
        }
    } else if raw.header.dwType == RIM_TYPEKEYBOARD.0 {
        let kb = raw.data.keyboard;
        let vkey = kb.VKey;
        let flags = kb.Flags;

        // Detect Right Alt (AltGr):
        // Right Alt sends VK_MENU or VK_RMENU with the E0 extended-key flag set.
        let is_right_alt = vkey == VK_RMENU.0 || (vkey == VK_MENU.0 && (flags & RI_KEY_E0) != 0);

        if is_right_alt {
            let is_release = (flags & RI_KEY_BREAK) != 0;
            if state.enable_alt.load(Ordering::Relaxed) {
                if is_release {
                    state.alt_trigger.on_release(&state.tx);
                } else {
                    // Send a dummy keystroke (Ctrl) to prevent Windows from activating the window menu when Alt is released.
                    crate::keyboard::send_key_click(crate::keyboard::Key::Control);
                    state.alt_trigger.on_press(&state.tx);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// HoldTrigger — identical logic to the original implementation
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct HoldTrigger {
    pressed: Arc<AtomicBool>,
    skill_active: Arc<AtomicBool>,
    generation: Arc<AtomicU64>,
}

impl HoldTrigger {
    fn new() -> Self {
        Self {
            pressed: Arc::new(AtomicBool::new(false)),
            skill_active: Arc::new(AtomicBool::new(false)),
            generation: Arc::new(AtomicU64::new(0)),
        }
    }

    fn on_press(&self, tx: &Sender<InputEvent>) {
        if self.pressed.swap(true, Ordering::AcqRel) {
            return; // already pressed (repeat event)
        }

        self.skill_active.store(false, Ordering::Release);
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        let pressed = self.pressed.clone();
        let skill_active = self.skill_active.clone();
        let generation_state = self.generation.clone();
        let tx = tx.clone();

        thread::spawn(move || {
            thread::sleep(Duration::from_millis(HOLD_THRESHOLD_MS));

            if generation_state.load(Ordering::Acquire) != generation {
                return;
            }

            if !pressed.load(Ordering::Acquire) {
                return;
            }

            if !skill_active.swap(true, Ordering::AcqRel) {
                tx.send(InputEvent::StartSkill).ok();
            }
        });
    }

    fn on_release(&self, tx: &Sender<InputEvent>) {
        if !self.pressed.swap(false, Ordering::AcqRel) {
            return; // wasn't pressed
        }

        if self.skill_active.swap(false, Ordering::AcqRel) {
            tx.send(InputEvent::StopSkill).ok();
        } else {
            tx.send(InputEvent::Click).ok();
        }
    }
}
