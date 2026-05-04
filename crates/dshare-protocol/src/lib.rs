//! Wire protocol shared by server and client peers.
//!
//! Frame layout: `[u32 length BE][bincode-encoded Message]`.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub mod codec;
pub mod keycode;

pub const PROTOCOL_VERSION: u16 = 1;
pub const DEFAULT_PORT: u16 = 24800;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hello {
    pub protocol_version: u16,
    pub peer_id: Uuid,
    pub hostname: String,
    pub screen: ScreenInfo,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ScreenInfo {
    pub width: u32,
    pub height: u32,
}

/// All messages exchanged between peers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Message {
    Hello(Hello),
    HelloAck { accepted: bool, reason: Option<String> },

    /// Server tells client: cursor entered your screen at this position.
    EnterScreen { x: i32, y: i32 },
    /// Server tells client: cursor left, stop forwarding input.
    LeaveScreen,

    /// Relative mouse motion while cursor is on the remote screen.
    MouseMove { dx: i32, dy: i32 },
    /// Absolute warp (used after EnterScreen).
    MouseWarp { x: i32, y: i32 },
    MouseButton { button: MouseButton, pressed: bool },
    MouseWheel { dx: i32, dy: i32 },

    /// Raw key event. `keycode` follows a normalized table (see `keycode` module — TODO).
    KeyEvent { keycode: u32, pressed: bool, modifiers: KeyModifiers },

    /// Clipboard sync. Payload is plain UTF-8 for now; binary formats land later.
    ClipboardUpdate { mime: String, data: Vec<u8> },

    /// Heartbeat for liveness detection.
    Ping { nonce: u64 },
    Pong { nonce: u64 },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Back,
    Forward,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct KeyModifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("encode: {0}")]
    Encode(Box<bincode::ErrorKind>),
    #[error("frame too large: {0} bytes")]
    FrameTooLarge(usize),
    #[error("incompatible protocol version: peer={peer} ours={ours}")]
    VersionMismatch { peer: u16, ours: u16 },
}

impl From<Box<bincode::ErrorKind>> for ProtocolError {
    fn from(e: Box<bincode::ErrorKind>) -> Self {
        ProtocolError::Encode(e)
    }
}
