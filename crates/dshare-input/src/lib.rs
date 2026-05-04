//! Cross-platform input capture and injection.
//!
//! Two traits the rest of the system speaks to:
//! - `InputCapture`: read local mouse/keyboard events (server side)
//! - `InputInject`: synthesize events from the wire (client side)
//!
//! Implementations are picked at compile time via `cfg`.

use async_trait::async_trait;
use dshare_protocol::Message;
use tokio::sync::mpsc;

#[async_trait]
pub trait InputCapture: Send {
    /// Start capturing. While `grabbed` is true, local input is consumed
    /// (not delivered to the OS) and forwarded as `Message`s on `out`.
    async fn run(&mut self, out: mpsc::Sender<Message>) -> anyhow::Result<()>;

    /// Toggle whether captured events are also blocked from reaching local apps.
    fn set_grabbed(&mut self, grabbed: bool);
}

#[async_trait]
pub trait InputInject: Send {
    async fn handle(&mut self, msg: &Message) -> anyhow::Result<()>;
}

#[cfg(windows)]
pub mod windows;
#[cfg(target_os = "linux")]
pub mod linux;

/// Construct the platform-default capture backend.
pub fn default_capture() -> anyhow::Result<Box<dyn InputCapture>> {
    #[cfg(windows)]
    { return Ok(Box::new(windows::WinCapture::new()?)); }
    #[cfg(target_os = "linux")]
    { return Ok(Box::new(linux::LinuxCapture::new()?)); }
    #[allow(unreachable_code)]
    { anyhow::bail!("no capture backend for this platform") }
}

/// Construct the platform-default inject backend.
pub fn default_inject() -> anyhow::Result<Box<dyn InputInject>> {
    #[cfg(windows)]
    { return Ok(Box::new(windows::WinInject::new()?)); }
    #[cfg(target_os = "linux")]
    { return Ok(Box::new(linux::LinuxInject::new()?)); }
    #[allow(unreachable_code)]
    { anyhow::bail!("no inject backend for this platform") }
}
