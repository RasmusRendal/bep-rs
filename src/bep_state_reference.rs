use crate::bep_state::BepState;
use crate::models::Peer;
use crate::sync_directory::{SyncDirectory, SyncFile};
use futures::channel::oneshot;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::Receiver;
use tokio::sync::mpsc::{channel, Sender};

enum BepStateCommand {
    GetSyncDirectories(oneshot::Sender<Vec<SyncDirectory>>),
    IsDirectorySynced(String, i32, oneshot::Sender<bool>),
    UpdateSyncFile(SyncDirectory, SyncFile, oneshot::Sender<()>),
    AddSyncDirectory(PathBuf, Option<String>, oneshot::Sender<SyncDirectory>),
}

#[derive(Clone)]
pub struct BepStateRef {
    pub state: Arc<Mutex<BepState>>,
    sender: Sender<BepStateCommand>,
}

async fn handle_commands(mut rx: Receiver<BepStateCommand>, state: Arc<Mutex<BepState>>) {
    while let Some(cmd) = rx.recv().await {
        match cmd {
            BepStateCommand::GetSyncDirectories(sender) => {
                let _ = sender.send(state.lock().unwrap().get_sync_directories());
            }
            BepStateCommand::IsDirectorySynced(directory, peer, sender) => {
                let _ = sender.send(state.lock().unwrap().is_directory_synced(directory, peer));
            }
            BepStateCommand::UpdateSyncFile(directory, file, sender) => {
                let _ = sender.send(state.lock().unwrap().update_sync_file(&directory, &file));
            }
            BepStateCommand::AddSyncDirectory(path, id, sender) => {
                let _ = sender.send(state.lock().unwrap().add_sync_directory(path, id));
            }
        }
    }
    rx.close();
}

impl BepStateRef {
    pub fn new(state: Arc<Mutex<BepState>>) -> Self {
        let (sender, receiver) = channel(100);
        let sc = state.clone();
        tokio::spawn(async move {
            handle_commands(receiver, sc).await;
        });
        BepStateRef { state, sender }
    }

    pub fn from_state(state: BepState) -> Self {
        BepStateRef::new(Arc::new(Mutex::new(state)))
    }

    pub async fn get_sync_directories(&self) -> Vec<SyncDirectory> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(BepStateCommand::GetSyncDirectories(tx))
            .await
            .unwrap();
        rx.await.unwrap()
    }

    pub async fn get_sync_directory(&self, id: &String) -> Option<SyncDirectory> {
        // TODO: Handle better
        self.get_sync_directories()
            .await
            .into_iter()
            .find(|dir| dir.id == *id)
    }

    pub async fn add_sync_directory(&mut self, path: PathBuf, id: Option<String>) -> SyncDirectory {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(BepStateCommand::AddSyncDirectory(path, id, tx))
            .await
            .unwrap();
        rx.await.unwrap()
    }

    pub async fn is_directory_synced(&self, directory: &SyncDirectory, peer: &Peer) -> bool {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(BepStateCommand::IsDirectorySynced(
                directory.id.clone(),
                peer.id.unwrap(),
                tx,
            ))
            .await
            .unwrap();
        rx.await.unwrap()
    }

    pub async fn update_sync_file(&self, directory: SyncDirectory, file: SyncFile) {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(BepStateCommand::UpdateSyncFile(directory, file, tx))
            .await
            .unwrap();
        rx.await.unwrap()
    }

    pub async fn get_short_id(&self) -> u64 {
        self.state.lock().unwrap().get_short_id()
    }
}
