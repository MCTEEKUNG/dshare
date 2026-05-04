//! Per-peer TCP session: framed read/write loops over `MessageCodec`.

use dshare_protocol::{codec::MessageCodec, Message, ProtocolError};
use futures::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_util::codec::Framed;
use tracing::{debug, warn};

pub struct PeerSession {
    pub outbound: mpsc::Sender<Message>,
}

impl PeerSession {
    /// Spawn read/write tasks for `stream`. Inbound messages are forwarded to
    /// `inbound`; messages sent on the returned `outbound` are written to the wire.
    pub fn spawn(stream: TcpStream, inbound: mpsc::Sender<Message>) -> Self {
        let framed = Framed::new(stream, MessageCodec);
        let (mut sink, mut source) = framed.split();
        let (tx, mut rx) = mpsc::channel::<Message>(256);

        tokio::spawn(async move {
            while let Some(frame) = source.next().await {
                match frame {
                    Ok(msg) => {
                        if inbound.send(msg).await.is_err() {
                            debug!("inbound receiver dropped, closing read loop");
                            break;
                        }
                    }
                    Err(ProtocolError::Io(e)) => {
                        debug!("peer disconnected: {e}");
                        break;
                    }
                    Err(e) => {
                        warn!("decode error: {e}");
                        break;
                    }
                }
            }
        });

        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                if let Err(e) = sink.send(msg).await {
                    warn!("write error: {e}");
                    break;
                }
            }
        });

        PeerSession { outbound: tx }
    }
}
