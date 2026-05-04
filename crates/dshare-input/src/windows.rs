//! Windows backend.
//!
//! Capture: low-level WH_MOUSE_LL / WH_KEYBOARD_LL hooks. The hook callback
//! must run on a thread with a message pump; we own a dedicated thread for it.
//!
//! Inject: `SendInput` for both mouse and keyboard.
//!
//! NOTE: This is a skeleton — hook installation and the message pump are
//! stubbed. Filling them in is the next milestone for the server side on
//! Windows.

use async_trait::async_trait;
use dshare_protocol::{Message, MouseButton};
use tokio::sync::mpsc;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
    MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN,
    MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL, MOUSEINPUT, MOUSE_EVENT_FLAGS,
};

use crate::{InputCapture, InputInject};

pub struct WinCapture {
    grabbed: bool,
}

impl WinCapture {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self { grabbed: false })
    }
}

#[async_trait]
impl InputCapture for WinCapture {
    async fn run(&mut self, _out: mpsc::Sender<Message>) -> anyhow::Result<()> {
        // TODO: spawn dedicated OS thread that calls SetWindowsHookExW with
        // WH_MOUSE_LL + WH_KEYBOARD_LL, runs GetMessage pump, and forwards
        // events through `_out`. The hook callback returns 1 to swallow when
        // `self.grabbed` is true.
        anyhow::bail!("WinCapture::run not yet implemented")
    }

    fn set_grabbed(&mut self, grabbed: bool) {
        self.grabbed = grabbed;
    }
}

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
                    _ => return Ok(()), // back/forward: TODO via XBUTTON
                };
                self.send_mouse(flags, 0, 0, 0);
            }
            Message::MouseWheel { dy, .. } => {
                self.send_mouse(MOUSEEVENTF_WHEEL, 0, 0, *dy * 120);
            }
            Message::KeyEvent { .. } => {
                // TODO: keybd_event / SendInput KEYBDINPUT with scan codes.
            }
            _ => {}
        }
        Ok(())
    }
}
