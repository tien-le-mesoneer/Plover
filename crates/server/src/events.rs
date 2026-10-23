//! Server → UI notifications over `GET /api/events` (WebSocket, same token subprotocol
//! as runs): the desktop shell tells the page about things it did natively.

use serde::Serialize;
use tokio::sync::broadcast;
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerEvent {
    /// A collection was opened outside the page (File → Open Collection…, a second
    /// launch with a path); the UI adds and selects it without reloading.
    CollectionOpened { id: String },
    /// The user chose Reload Window; the UI reloads, asking first if anything is unsaved.
    ReloadRequested,
}

#[derive(Clone)]
pub struct Events {
    tx: broadcast::Sender<ServerEvent>,
}

impl Default for Events {
    fn default() -> Events {
        Events {
            tx: broadcast::channel(64).0,
        }
    }
}

impl Events {
    /// Delivers `event` to every connected page (none connected: dropped).
    pub fn send(&self, event: ServerEvent) {
        let _ = self.tx.send(event);
    }

    /// Pages currently listening.
    pub fn receivers(&self) -> usize {
        self.tx.receiver_count()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ServerEvent> {
        self.tx.subscribe()
    }
}
