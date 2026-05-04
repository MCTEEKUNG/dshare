//! Multi-screen layout: where each peer's screen sits relative to the server.
//!
//! Coordinate model: server screen is the origin (0,0) at top-left, extending
//! `width × height`. Other screens are placed by `Edge` (Left/Right/Top/Bottom)
//! relative to the server, each with its own size. Cursor "leaves" through an
//! edge when it crosses x<0, x>=w, y<0, y>=h.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Layout {
    pub server_screen: Screen,
    #[serde(default)]
    pub peers: Vec<PeerScreen>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Screen {
    pub width: u32,
    pub height: u32,
}

impl Default for Screen {
    fn default() -> Self {
        Self { width: 1920, height: 1080 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerScreen {
    pub peer_id: Uuid,
    pub name: String,
    pub edge: Edge,
    /// Offset along the shared edge, in pixels. 0 means flush at the top
    /// (for Left/Right) or flush at the left (for Top/Bottom).
    pub offset: i32,
    pub screen: Screen,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

/// Result of a cursor movement check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeCross {
    Stay,
    Cross { peer_index: usize, entry_x: i32, entry_y: i32 },
}

impl Layout {
    /// Determine if cursor at `(x, y)` on the server screen crosses to a peer.
    /// Returns the peer's local entry coordinates clamped to the peer's size.
    pub fn check_edge(&self, x: i32, y: i32) -> EdgeCross {
        let w = self.server_screen.width as i32;
        let h = self.server_screen.height as i32;

        for (i, peer) in self.peers.iter().enumerate() {
            let pw = peer.screen.width as i32;
            let ph = peer.screen.height as i32;

            let crossed = match peer.edge {
                Edge::Left if x < 0 => {
                    let local_y = (y - peer.offset).clamp(0, ph - 1);
                    Some((pw - 1, local_y))
                }
                Edge::Right if x >= w => {
                    let local_y = (y - peer.offset).clamp(0, ph - 1);
                    Some((0, local_y))
                }
                Edge::Top if y < 0 => {
                    let local_x = (x - peer.offset).clamp(0, pw - 1);
                    Some((local_x, ph - 1))
                }
                Edge::Bottom if y >= h => {
                    let local_x = (x - peer.offset).clamp(0, pw - 1);
                    Some((local_x, 0))
                }
                _ => None,
            };

            if let Some((ex, ey)) = crossed {
                return EdgeCross::Cross { peer_index: i, entry_x: ex, entry_y: ey };
            }
        }
        EdgeCross::Stay
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout_with_right_peer() -> Layout {
        Layout {
            server_screen: Screen { width: 1920, height: 1080 },
            peers: vec![PeerScreen {
                peer_id: Uuid::nil(),
                name: "ubuntu".into(),
                edge: Edge::Right,
                offset: 0,
                screen: Screen { width: 2560, height: 1440 },
            }],
        }
    }

    #[test]
    fn cursor_within_server_stays() {
        let l = layout_with_right_peer();
        assert_eq!(l.check_edge(100, 100), EdgeCross::Stay);
    }

    #[test]
    fn cursor_crosses_right_edge() {
        let l = layout_with_right_peer();
        match l.check_edge(1920, 500) {
            EdgeCross::Cross { peer_index: 0, entry_x: 0, entry_y: 500 } => {}
            other => panic!("expected right cross, got {other:?}"),
        }
    }

    #[test]
    fn cursor_clamped_below_peer_height() {
        let l = layout_with_right_peer();
        // Server is 1080 tall, peer is 1440 — y=1079 maps to 1079 on peer.
        if let EdgeCross::Cross { entry_y, .. } = l.check_edge(1920, 1079) {
            assert_eq!(entry_y, 1079);
        } else {
            panic!("expected cross");
        }
    }
}
