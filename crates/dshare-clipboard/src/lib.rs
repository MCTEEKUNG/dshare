//! Clipboard polling + apply.
//!
//! `arboard` doesn't expose change events, so we poll. A small hash avoids
//! re-broadcasting unchanged content. Per-direction loop suppression prevents
//! echo when we apply a remote update locally.

use dshare_protocol::Message;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, warn};

const POLL_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Default)]
struct State {
    last_hash: u64,
}

pub struct Clipboard {
    inner: Arc<Mutex<arboard::Clipboard>>,
    state: Arc<Mutex<State>>,
}

impl Clipboard {
    pub fn new() -> anyhow::Result<Self> {
        let inner = arboard::Clipboard::new()?;
        Ok(Self {
            inner: Arc::new(Mutex::new(inner)),
            state: Arc::new(Mutex::new(State::default())),
        })
    }

    /// Spawn a polling task that emits `Message::ClipboardUpdate` on change.
    pub fn spawn_watcher(&self, out: mpsc::Sender<Message>) {
        let inner = Arc::clone(&self.inner);
        let state = Arc::clone(&self.state);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(POLL_INTERVAL).await;
                let text = match inner.lock().unwrap().get_text() {
                    Ok(t) => t,
                    Err(arboard::Error::ContentNotAvailable) => continue,
                    Err(e) => {
                        debug!("clipboard read failed: {e}");
                        continue;
                    }
                };
                let h = hash_str(&text);
                {
                    let mut s = state.lock().unwrap();
                    if s.last_hash == h {
                        continue;
                    }
                    s.last_hash = h;
                }
                let msg = Message::ClipboardUpdate {
                    mime: "text/plain;charset=utf-8".into(),
                    data: text.into_bytes(),
                };
                if out.send(msg).await.is_err() {
                    break;
                }
            }
        });
    }

    /// Apply an inbound clipboard message locally without retriggering broadcast.
    pub fn apply(&self, msg: &Message) {
        let Message::ClipboardUpdate { mime, data } = msg else { return };
        if !mime.starts_with("text/") {
            return; // binary types: TODO
        }
        let Ok(text) = std::str::from_utf8(data) else {
            warn!("non-utf8 text/* clipboard payload, skipping");
            return;
        };
        let h = hash_str(text);
        {
            let mut s = self.state.lock().unwrap();
            s.last_hash = h; // suppress echo on next poll
        }
        if let Err(e) = self.inner.lock().unwrap().set_text(text.to_string()) {
            warn!("clipboard write failed: {e}");
        }
    }
}

fn hash_str(s: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}
