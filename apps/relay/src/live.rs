//! Waking subscribers: one channel per family that carries no data, only
//! "there is something new, look again".

use std::{collections::HashMap, sync::Arc};

use tokio::sync::{Mutex, broadcast};
use uuid::Uuid;

#[derive(Clone, Default)]
pub struct Live {
    channels: Arc<Mutex<HashMap<Uuid, broadcast::Sender<()>>>>,
}

impl Live {
    pub async fn subscribe(&self, family_id: Uuid) -> broadcast::Receiver<()> {
        self.channels
            .lock()
            .await
            .entry(family_id)
            .or_insert_with(|| broadcast::channel(64).0)
            .subscribe()
    }

    pub async fn notify(&self, family_id: Uuid) {
        if let Some(sender) = self.channels.lock().await.get(&family_id) {
            // No receivers is fine: nobody is watching.
            let _ = sender.send(());
        }
    }
}
