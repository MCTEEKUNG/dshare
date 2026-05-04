//! Linux backend.
//!
//! Capture: read from `/dev/input/event*` via `evdev`. Requires the user be
//! in the `input` group (or run as root). Wayland note: capture works fine
//! since we read raw devices, but injection on Wayland needs `uinput`, which
//! we use here for both X11 and Wayland.
//!
//! Inject: create a `uinput` virtual device with mouse + keyboard caps.
//!
//! ## Permissions
//! `/dev/uinput` is root-only by default. Either run as root, or install a
//! udev rule like:
//! ```text
//! # /etc/udev/rules.d/99-dshare-uinput.rules
//! KERNEL=="uinput", GROUP="input", MODE="0660"
//! ```
//! and ensure the user is in the `input` group.
//!
//! ## Keycode convention
//! The wire `Message::KeyEvent.keycode` is interpreted directly as a Linux
//! evdev key code (u16, see <linux/input-event-codes.h>). The Windows
//! capture backend translates VK → evdev before sending.

use async_trait::async_trait;
use dshare_protocol::{Message, MouseButton};
use evdev::{
    uinput::{VirtualDevice, VirtualDeviceBuilder},
    AttributeSet, EventType, InputEvent, Key, RelativeAxisType,
};
use tokio::sync::mpsc;
use tracing::warn;

use crate::{InputCapture, InputInject};

pub struct LinuxCapture {
    grabbed: bool,
}

impl LinuxCapture {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self { grabbed: false })
    }
}

#[async_trait]
impl InputCapture for LinuxCapture {
    async fn run(&mut self, _out: mpsc::Sender<Message>) -> anyhow::Result<()> {
        // TODO: enumerate /dev/input/event*, pick devices with EV_REL+BTN_LEFT
        // (mouse) and EV_KEY (keyboard), call `device.grab()` while
        // `self.grabbed` to consume events, forward translated Messages on _out.
        anyhow::bail!("LinuxCapture::run not yet implemented")
    }

    fn set_grabbed(&mut self, grabbed: bool) {
        self.grabbed = grabbed;
    }
}

pub struct LinuxInject {
    device: VirtualDevice,
}

impl LinuxInject {
    pub fn new() -> anyhow::Result<Self> {
        let mut keys = AttributeSet::<Key>::new();
        // Keyboard range: KEY_ESC (1) .. KEY_MICMUTE (248). Covers everything
        // a typical layout produces. Codes outside this range are ignored.
        for code in 1u16..=248u16 {
            keys.insert(Key(code));
        }
        // Mouse buttons: BTN_MISC (0x100) .. BTN_GEAR_UP (0x150).
        for code in 0x100u16..=0x150u16 {
            keys.insert(Key(code));
        }

        let mut rels = AttributeSet::<RelativeAxisType>::new();
        rels.insert(RelativeAxisType::REL_X);
        rels.insert(RelativeAxisType::REL_Y);
        rels.insert(RelativeAxisType::REL_WHEEL);
        rels.insert(RelativeAxisType::REL_HWHEEL);

        let device = VirtualDeviceBuilder::new()
            .map_err(|e| anyhow::anyhow!(
                "opening /dev/uinput failed (need root or 'input' group + udev rule): {e}"
            ))?
            .name("dshare virtual input")
            .with_keys(&keys)?
            .with_relative_axes(&rels)?
            .build()?;

        Ok(Self { device })
    }
}

#[async_trait]
impl InputInject for LinuxInject {
    async fn handle(&mut self, msg: &Message) -> anyhow::Result<()> {
        let mut events: Vec<InputEvent> = Vec::with_capacity(2);
        match msg {
            Message::MouseMove { dx, dy } => {
                if *dx != 0 {
                    events.push(InputEvent::new(
                        EventType::RELATIVE,
                        RelativeAxisType::REL_X.0,
                        *dx,
                    ));
                }
                if *dy != 0 {
                    events.push(InputEvent::new(
                        EventType::RELATIVE,
                        RelativeAxisType::REL_Y.0,
                        *dy,
                    ));
                }
            }
            Message::MouseButton { button, pressed } => {
                let key = match button {
                    MouseButton::Left => Key::BTN_LEFT,
                    MouseButton::Right => Key::BTN_RIGHT,
                    MouseButton::Middle => Key::BTN_MIDDLE,
                    MouseButton::Back => Key::BTN_SIDE,
                    MouseButton::Forward => Key::BTN_EXTRA,
                };
                events.push(InputEvent::new(
                    EventType::KEY,
                    key.0,
                    if *pressed { 1 } else { 0 },
                ));
            }
            Message::MouseWheel { dx, dy } => {
                if *dy != 0 {
                    events.push(InputEvent::new(
                        EventType::RELATIVE,
                        RelativeAxisType::REL_WHEEL.0,
                        *dy,
                    ));
                }
                if *dx != 0 {
                    events.push(InputEvent::new(
                        EventType::RELATIVE,
                        RelativeAxisType::REL_HWHEEL.0,
                        *dx,
                    ));
                }
            }
            Message::KeyEvent { keycode, pressed, .. } => {
                if *keycode > u16::MAX as u32 {
                    warn!("evdev key code {keycode} out of u16 range, dropping");
                    return Ok(());
                }
                events.push(InputEvent::new(
                    EventType::KEY,
                    *keycode as u16,
                    if *pressed { 1 } else { 0 },
                ));
            }
            // MouseWarp is absolute and only meaningful right after EnterScreen.
            // uinput-via-REL can't warp directly; client handles by accumulating
            // current position and emitting deltas. See `handle_peer` in dshare/main.rs.
            _ => return Ok(()),
        }

        if !events.is_empty() {
            self.device.emit(&events)?;
        }
        Ok(())
    }
}
