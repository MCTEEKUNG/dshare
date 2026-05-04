//! Windows backend.
//!
//! ## Capture
//! Low-level WH_MOUSE_LL / WH_KEYBOARD_LL hooks, installed on a dedicated
//! std::thread that runs a Windows message pump. The C-style hook callbacks
//! cannot capture state, so the per-thread channel sender and shared
//! grab flag live in `thread_local` cells.
//!
//! Hook callbacks must return quickly — Windows silently disables a hook
//! whose callback exceeds `LowLevelHooksTimeout` (default 300ms). We use
//! `try_send` to avoid blocking; events drop if the consumer is slow.
//!
//! ## Inject
//! `SendInput` for both mouse and keyboard.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::thread;

use async_trait::async_trait;
use dshare_protocol::{keycode, KeyModifiers, Message, MouseButton};
use tokio::sync::mpsc;
use tracing::{debug, error, warn};

use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, MapVirtualKeyW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE,
    KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, MAPVK_VK_TO_VSC,
    MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP,
    MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL, MOUSEINPUT,
    MOUSE_EVENT_FLAGS, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, PostThreadMessageW, SetWindowsHookExW,
    TranslateMessage, UnhookWindowsHookEx, HC_ACTION, KBDLLHOOKSTRUCT, MSG, MSLLHOOKSTRUCT,
    WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_QUIT, WM_RBUTTONDOWN,
    WM_RBUTTONUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use crate::{InputCapture, InputInject};

// State accessed from the C-style hook callbacks. Lives on the hook thread.
thread_local! {
    static SENDER: RefCell<Option<mpsc::Sender<Message>>> = const { RefCell::new(None) };
    static GRABBED: RefCell<Option<Arc<AtomicBool>>> = const { RefCell::new(None) };
    static LAST_MOUSE_POS: RefCell<Option<(i32, i32)>> = const { RefCell::new(None) };
}

pub struct WinCapture {
    grabbed: Arc<AtomicBool>,
    /// Set once the hook thread starts up so we can post WM_QUIT for shutdown.
    hook_thread_id: Arc<AtomicU32>,
    hook_thread: Option<thread::JoinHandle<()>>,
}

impl WinCapture {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            grabbed: Arc::new(AtomicBool::new(false)),
            hook_thread_id: Arc::new(AtomicU32::new(0)),
            hook_thread: None,
        })
    }
}

impl Drop for WinCapture {
    fn drop(&mut self) {
        let tid = self.hook_thread_id.load(Ordering::SeqCst);
        if tid != 0 {
            unsafe {
                let _ = PostThreadMessageW(tid, WM_QUIT, WPARAM(0), LPARAM(0));
            }
        }
        if let Some(handle) = self.hook_thread.take() {
            let _ = handle.join();
        }
    }
}

#[async_trait]
impl InputCapture for WinCapture {
    async fn run(&mut self, out: mpsc::Sender<Message>) -> anyhow::Result<()> {
        let grabbed = Arc::clone(&self.grabbed);
        let tid_slot = Arc::clone(&self.hook_thread_id);

        let handle = thread::Builder::new()
            .name("dshare-hook".into())
            .spawn(move || hook_thread_main(out, grabbed, tid_slot))?;
        self.hook_thread = Some(handle);

        // Park forever — the hook thread does the real work. The caller
        // drops `self` (which posts WM_QUIT) for a clean shutdown.
        std::future::pending::<()>().await;
        Ok(())
    }

    fn set_grabbed(&mut self, grabbed: bool) {
        self.grabbed.store(grabbed, Ordering::SeqCst);
    }
}

fn hook_thread_main(
    sender: mpsc::Sender<Message>,
    grabbed: Arc<AtomicBool>,
    tid_slot: Arc<AtomicU32>,
) {
    SENDER.with(|s| *s.borrow_mut() = Some(sender));
    GRABBED.with(|g| *g.borrow_mut() = Some(grabbed));

    // Publish our thread id so the owner can signal WM_QUIT.
    let tid = unsafe { windows::Win32::System::Threading::GetCurrentThreadId() };
    tid_slot.store(tid, Ordering::SeqCst);

    let mouse_hook = match unsafe {
        SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook_proc), HINSTANCE::default(), 0)
    } {
        Ok(h) => h,
        Err(e) => {
            error!("SetWindowsHookExW(WH_MOUSE_LL) failed: {e}");
            return;
        }
    };

    let kbd_hook = match unsafe {
        SetWindowsHookExW(WH_KEYBOARD_LL, Some(kbd_hook_proc), HINSTANCE::default(), 0)
    } {
        Ok(h) => h,
        Err(e) => {
            error!("SetWindowsHookExW(WH_KEYBOARD_LL) failed: {e}");
            unsafe {
                let _ = UnhookWindowsHookEx(mouse_hook);
            }
            return;
        }
    };

    debug!("hooks installed (tid={tid}), entering message pump");

    let mut msg = MSG::default();
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    debug!("message pump exited, removing hooks");
    unsafe {
        let _ = UnhookWindowsHookEx(mouse_hook);
        let _ = UnhookWindowsHookEx(kbd_hook);
    }
}

extern "system" fn mouse_hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code != HC_ACTION as i32 {
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }

    // Safety: lparam points to a kernel-supplied MSLLHOOKSTRUCT for the duration
    // of this callback.
    let info = unsafe { *(lparam.0 as *const MSLLHOOKSTRUCT) };
    let cur_pos = (info.pt.x, info.pt.y);

    let msg = match wparam.0 as u32 {
        WM_MOUSEMOVE => {
            let last = LAST_MOUSE_POS.with(|l| *l.borrow());
            LAST_MOUSE_POS.with(|l| *l.borrow_mut() = Some(cur_pos));
            last.map(|(lx, ly)| Message::MouseMove {
                dx: cur_pos.0 - lx,
                dy: cur_pos.1 - ly,
            })
        }
        WM_LBUTTONDOWN => Some(Message::MouseButton {
            button: MouseButton::Left,
            pressed: true,
        }),
        WM_LBUTTONUP => Some(Message::MouseButton {
            button: MouseButton::Left,
            pressed: false,
        }),
        WM_RBUTTONDOWN => Some(Message::MouseButton {
            button: MouseButton::Right,
            pressed: true,
        }),
        WM_RBUTTONUP => Some(Message::MouseButton {
            button: MouseButton::Right,
            pressed: false,
        }),
        WM_MBUTTONDOWN => Some(Message::MouseButton {
            button: MouseButton::Middle,
            pressed: true,
        }),
        WM_MBUTTONUP => Some(Message::MouseButton {
            button: MouseButton::Middle,
            pressed: false,
        }),
        WM_MOUSEWHEEL => {
            // High word of mouseData is a signed wheel delta in multiples of WHEEL_DELTA (120).
            let delta = (info.mouseData >> 16) as u16 as i16;
            Some(Message::MouseWheel {
                dx: 0,
                dy: (delta / 120) as i32,
            })
        }
        _ => None,
    };

    if let Some(msg) = msg {
        SENDER.with(|s| {
            if let Some(s) = s.borrow().as_ref() {
                let _ = s.try_send(msg);
            }
        });
    }

    let grab = GRABBED.with(|g| {
        g.borrow()
            .as_ref()
            .map(|a| a.load(Ordering::Relaxed))
            .unwrap_or(false)
    });
    if grab {
        LRESULT(1) // swallow event
    } else {
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    }
}

extern "system" fn kbd_hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code != HC_ACTION as i32 {
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }

    let info = unsafe { *(lparam.0 as *const KBDLLHOOKSTRUCT) };
    let w = wparam.0 as u32;

    let pressed = matches!(w, WM_KEYDOWN | WM_SYSKEYDOWN);
    let is_key = matches!(w, WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN | WM_SYSKEYUP);

    if is_key {
        if let Some(evdev) = keycode::vk_to_evdev(info.vkCode) {
            let msg = Message::KeyEvent {
                keycode: evdev as u32,
                pressed,
                modifiers: read_current_modifiers(),
            };
            SENDER.with(|s| {
                if let Some(s) = s.borrow().as_ref() {
                    let _ = s.try_send(msg);
                }
            });
        } else {
            warn!("unmapped vk code: {:#x}", info.vkCode);
        }
    }

    let grab = GRABBED.with(|g| {
        g.borrow()
            .as_ref()
            .map(|a| a.load(Ordering::Relaxed))
            .unwrap_or(false)
    });
    if grab {
        LRESULT(1)
    } else {
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    }
}

fn read_current_modifiers() -> KeyModifiers {
    fn down(vk: VIRTUAL_KEY) -> bool {
        unsafe { (GetAsyncKeyState(vk.0 as i32) as u16 & 0x8000) != 0 }
    }
    KeyModifiers {
        shift: down(VK_SHIFT),
        ctrl: down(VK_CONTROL),
        alt: down(VK_MENU),
        meta: down(VK_LWIN) || down(VK_RWIN),
    }
}

// ---------------------------------------------------------------------------
// Inject
// ---------------------------------------------------------------------------

pub struct WinInject;

impl WinInject {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self)
    }

    fn send_mouse(&self, flags: MOUSE_EVENT_FLAGS, dx: i32, dy: i32, data: i32) {
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx,
                    dy,
                    mouseData: data as u32,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        unsafe {
            SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
        }
    }

    fn send_key(&self, evdev_code: u16, pressed: bool) {
        // Inverse of vk_to_evdev. For now we only need a partial inverse since
        // tests typically synthesize the keys we know about. A proper inverse
        // table can replace this — left as TODO.
        let Some(vk) = evdev_to_vk(evdev_code) else {
            warn!("evdev code {evdev_code} has no vk mapping");
            return;
        };

        // Convert VK → scan code for portability across keyboard layouts.
        let scan = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) } as u16;

        let mut flags = KEYEVENTF_SCANCODE;
        if !pressed {
            flags |= KEYEVENTF_KEYUP;
        }
        if is_extended_vk(vk) {
            flags |= KEYEVENTF_EXTENDEDKEY;
        }

        let input = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0),
                    wScan: scan,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        unsafe {
            SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
        }
    }
}

fn is_extended_vk(vk: u16) -> bool {
    matches!(
        vk,
        // Arrows, navigation block, numpad enter/divide, right modifiers, etc.
        0x21 | 0x22 | 0x23 | 0x24 | 0x25 | 0x26 | 0x27 | 0x28 |
        0x2D | 0x2E |
        0x6F | 0x0D | 0xA3 | 0xA5 | 0x5B | 0x5C
    )
}

fn evdev_to_vk(code: u16) -> Option<u16> {
    Some(match code {
        // Letters
        30 => 0x41, 48 => 0x42, 46 => 0x43, 32 => 0x44, 18 => 0x45,
        33 => 0x46, 34 => 0x47, 35 => 0x48, 23 => 0x49, 36 => 0x4A,
        37 => 0x4B, 38 => 0x4C, 50 => 0x4D, 49 => 0x4E, 24 => 0x4F,
        25 => 0x50, 16 => 0x51, 19 => 0x52, 31 => 0x53, 20 => 0x54,
        22 => 0x55, 47 => 0x56, 17 => 0x57, 45 => 0x58, 21 => 0x59, 44 => 0x5A,
        // Digits
        11 => 0x30, 2 => 0x31, 3 => 0x32, 4 => 0x33, 5 => 0x34,
        6 => 0x35, 7 => 0x36, 8 => 0x37, 9 => 0x38, 10 => 0x39,
        // Whitespace / control
        28 => 0x0D, 1 => 0x1B, 14 => 0x08, 15 => 0x09, 57 => 0x20,
        // Arrows
        105 => 0x25, 103 => 0x26, 106 => 0x27, 108 => 0x28,
        // Modifiers
        42 => 0xA0, 54 => 0xA1, 29 => 0xA2, 97 => 0xA3, 56 => 0xA4, 100 => 0xA5,
        125 => 0x5B, 126 => 0x5C,
        // Function keys
        59 => 0x70, 60 => 0x71, 61 => 0x72, 62 => 0x73, 63 => 0x74,
        64 => 0x75, 65 => 0x76, 66 => 0x77, 67 => 0x78, 68 => 0x79,
        87 => 0x7A, 88 => 0x7B,
        // Editing
        110 => 0x2D, 111 => 0x2E, 102 => 0x24, 107 => 0x23, 104 => 0x21, 109 => 0x22,
        // Locks
        58 => 0x14, 69 => 0x90, 70 => 0x91,
        _ => return None,
    })
}

#[async_trait]
impl InputInject for WinInject {
    async fn handle(&mut self, msg: &Message) -> anyhow::Result<()> {
        match msg {
            Message::MouseMove { dx, dy } => {
                self.send_mouse(MOUSEEVENTF_MOVE, *dx, *dy, 0);
            }
            Message::MouseButton { button, pressed } => {
                let flags = match (button, pressed) {
                    (MouseButton::Left, true) => MOUSEEVENTF_LEFTDOWN,
                    (MouseButton::Left, false) => MOUSEEVENTF_LEFTUP,
                    (MouseButton::Right, true) => MOUSEEVENTF_RIGHTDOWN,
                    (MouseButton::Right, false) => MOUSEEVENTF_RIGHTUP,
                    (MouseButton::Middle, true) => MOUSEEVENTF_MIDDLEDOWN,
                    (MouseButton::Middle, false) => MOUSEEVENTF_MIDDLEUP,
                    _ => return Ok(()), // back/forward via XBUTTON: TODO
                };
                self.send_mouse(flags, 0, 0, 0);
            }
            Message::MouseWheel { dy, .. } => {
                self.send_mouse(MOUSEEVENTF_WHEEL, 0, 0, *dy * 120);
            }
            Message::KeyEvent { keycode, pressed, .. } => {
                if *keycode <= u16::MAX as u32 {
                    self.send_key(*keycode as u16, *pressed);
                }
            }
            _ => {}
        }
        Ok(())
    }
}
