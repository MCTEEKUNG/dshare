//! Length-prefixed bincode framing for `Message`.

use bytes::{Buf, BufMut, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

use crate::{Message, ProtocolError};

/// 16 MiB hard cap — clipboard payloads can grow but should not be unbounded.
pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;

pub struct MessageCodec;

impl Decoder for MessageCodec {
    type Item = Message;
    type Error = ProtocolError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Message>, ProtocolError> {
        if src.len() < 4 {
            return Ok(None);
        }
        let len = u32::from_be_bytes(src[..4].try_into().unwrap()) as usize;
        if len > MAX_FRAME_BYTES {
            return Err(ProtocolError::FrameTooLarge(len));
        }
        if src.len() < 4 + len {
            src.reserve(4 + len - src.len());
            return Ok(None);
        }
        src.advance(4);
        let payload = src.split_to(len);
        let msg = bincode::deserialize(&payload)?;
        Ok(Some(msg))
    }
}

impl Encoder<Message> for MessageCodec {
    type Error = ProtocolError;

    fn encode(&mut self, item: Message, dst: &mut BytesMut) -> Result<(), ProtocolError> {
        let payload = bincode::serialize(&item)?;
        if payload.len() > MAX_FRAME_BYTES {
            return Err(ProtocolError::FrameTooLarge(payload.len()));
        }
        dst.reserve(4 + payload.len());
        dst.put_u32(payload.len() as u32);
        dst.extend_from_slice(&payload);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Hello, KeyModifiers, MouseButton, ScreenInfo};
    use uuid::Uuid;

    fn roundtrip(msg: Message) -> Message {
        let mut codec = MessageCodec;
        let mut buf = BytesMut::new();
        codec.encode(msg, &mut buf).expect("encode");
        codec.decode(&mut buf).expect("decode").expect("frame")
    }

    #[test]
    fn hello_roundtrip() {
        let original = Message::Hello(Hello {
            protocol_version: crate::PROTOCOL_VERSION,
            peer_id: Uuid::nil(),
            hostname: "test".into(),
            screen: ScreenInfo { width: 1920, height: 1080 },
        });
        match roundtrip(original) {
            Message::Hello(h) => {
                assert_eq!(h.hostname, "test");
                assert_eq!(h.screen.width, 1920);
            }
            other => panic!("expected Hello, got {other:?}"),
        }
    }

    #[test]
    fn mouse_events_roundtrip() {
        match roundtrip(Message::MouseMove { dx: -5, dy: 12 }) {
            Message::MouseMove { dx: -5, dy: 12 } => {}
            other => panic!("got {other:?}"),
        }
        match roundtrip(Message::MouseButton {
            button: MouseButton::Right,
            pressed: true,
        }) {
            Message::MouseButton { button: MouseButton::Right, pressed: true } => {}
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn key_event_roundtrip() {
        let mods = KeyModifiers { shift: true, ctrl: false, alt: false, meta: false };
        match roundtrip(Message::KeyEvent { keycode: 35, pressed: true, modifiers: mods }) {
            Message::KeyEvent { keycode: 35, pressed: true, modifiers } => {
                assert!(modifiers.shift);
                assert!(!modifiers.ctrl);
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn partial_frame_returns_none() {
        let mut codec = MessageCodec;
        let mut buf = BytesMut::new();
        codec
            .encode(Message::Ping { nonce: 42 }, &mut buf)
            .expect("encode");
        // Truncate to just the length prefix + 1 byte of payload.
        buf.truncate(5);
        assert!(codec.decode(&mut buf).expect("decode").is_none());
    }

    #[test]
    fn oversized_frame_rejected() {
        let mut codec = MessageCodec;
        let mut buf = BytesMut::new();
        // Forge a length prefix exceeding the cap.
        buf.put_u32((MAX_FRAME_BYTES + 1) as u32);
        match codec.decode(&mut buf) {
            Err(ProtocolError::FrameTooLarge(n)) => assert_eq!(n, MAX_FRAME_BYTES + 1),
            other => panic!("expected FrameTooLarge, got {other:?}"),
        }
    }

    #[test]
    fn two_messages_in_one_buffer() {
        let mut codec = MessageCodec;
        let mut buf = BytesMut::new();
        codec.encode(Message::Ping { nonce: 1 }, &mut buf).unwrap();
        codec.encode(Message::Pong { nonce: 1 }, &mut buf).unwrap();

        match codec.decode(&mut buf).unwrap().unwrap() {
            Message::Ping { nonce: 1 } => {}
            other => panic!("got {other:?}"),
        }
        match codec.decode(&mut buf).unwrap().unwrap() {
            Message::Pong { nonce: 1 } => {}
            other => panic!("got {other:?}"),
        }
        assert!(codec.decode(&mut buf).unwrap().is_none());
    }
}
