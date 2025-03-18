use crate::{
    bep_state::{BepState, NewFolderHandler},
    device_id::DeviceID,
    models::Peer,
    peer_connection::PeerConnection,
    sync_directory::{self, SyncDirectory},
};
use ring::signature::EcdsaKeyPair;
use std::{path::PathBuf, sync::Arc};
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct BepStateRef {
    state: Arc<Mutex<BepState>>,
}

// TODO: This whole thing could probably be a procedural macro
// This might also let us carry over documentation.
impl BepStateRef {
    pub fn from_bepstate(state: BepState) -> Self {
        BepStateRef {
            state: Arc::new(Mutex::new(state)),
        }
    }

    pub fn new(data_directory: PathBuf) -> Self {
        BepStateRef::from_bepstate(BepState::new(data_directory))
    }

    pub async fn get_directory_peers(&self, directory: &str) -> Vec<Peer> {
        self.state.lock().await.get_directory_peers(directory)
    }

    pub async fn get_synced_directories(
        &self,
        peer_id: i32,
    ) -> Vec<(sync_directory::SyncDirectory, Vec<Peer>)> {
        self.state.lock().await.get_synced_directories(peer_id)
    }

    pub async fn get_sync_directories(&self) -> Vec<sync_directory::SyncDirectory> {
        self.state.lock().await.get_sync_directories()
    }

    pub async fn set_sync_directory_path(
        &mut self,
        dir: &sync_directory::SyncDirectory,
        path: Option<PathBuf>,
    ) {
        self.state.lock().await.set_sync_directory_path(dir, path);
    }

    pub async fn get_sync_directory(&self, id: &String) -> Option<sync_directory::SyncDirectory> {
        self.state.lock().await.get_sync_directory(id)
    }

    pub async fn get_blocks(&self, file_id: i32) -> Vec<sync_directory::SyncBlock> {
        self.state.lock().await.get_blocks(file_id)
    }

    pub async fn get_sync_files(&self, dir_id: &String) -> Vec<sync_directory::SyncFile> {
        self.state.lock().await.get_sync_files(dir_id)
    }

    pub async fn update_sync_file(
        &self,
        dir: &sync_directory::SyncDirectory,
        file: &sync_directory::SyncFile,
    ) {
        self.state.lock().await.update_sync_file(dir, file)
    }

    pub async fn get_id(&self) -> DeviceID {
        self.state.lock().await.get_id()
    }

    pub async fn get_short_id(&self) -> u64 {
        self.state.lock().await.get_short_id()
    }

    pub async fn get_certificate(&self) -> Vec<u8> {
        self.state.lock().await.get_certificate()
    }

    pub async fn get_key(&self) -> Vec<u8> {
        self.state.lock().await.get_key()
    }

    pub async fn get_keypair(&self) -> EcdsaKeyPair {
        self.state.lock().await.get_keypair()
    }

    pub async fn get_name(&self) -> String {
        self.state.lock().await.get_name()
    }

    pub async fn set_name(&self, name: String) {
        self.state.lock().await.set_name(name)
    }

    pub async fn get_peers(&self) -> Vec<Peer> {
        self.state.lock().await.get_peers()
    }

    pub async fn get_peer_by_id(&self, dev_id: DeviceID) -> Peer {
        self.state.lock().await.get_peer_by_id(dev_id)
    }

    pub async fn get_addresses(&self, peer: Peer) -> Vec<String> {
        self.state.lock().await.get_addresses(peer)
    }

    pub async fn add_address(&self, peer: Peer, addr: String) {
        self.state.lock().await.add_address(peer, addr)
    }

    pub async fn remove_sync_directory(&self, to_remove: String) {
        self.state.lock().await.remove_sync_directory(to_remove)
    }

    pub async fn add_sync_directory(
        &self,
        path: Option<PathBuf>,
        label: String,
        id: Option<String>,
    ) -> sync_directory::SyncDirectory {
        self.state.lock().await.add_sync_directory(path, label, id)
    }

    pub async fn get_peer(&self, peer_name: String) -> Peer {
        self.state.lock().await.get_peer(peer_name)
    }

    pub async fn add_peer_vec_id(&self, peer_name: String, peer_id: Vec<u8>) -> Peer {
        self.state.lock().await.add_peer_vec_id(peer_name, peer_id)
    }

    pub async fn add_peer(&self, peer_name: String, peer_id: [u8; 32]) -> Peer {
        self.state.lock().await.add_peer(peer_name, peer_id)
    }

    pub async fn sync_directory_with_peer(
        &self,
        directory: &sync_directory::SyncDirectory,
        peer: &Peer,
    ) {
        self.state
            .lock()
            .await
            .sync_directory_with_peer(directory, peer)
    }

    pub async fn is_directory_synced(&self, directory: String, peer: i32) -> bool {
        self.state.lock().await.is_directory_synced(directory, peer)
    }

    pub async fn new_folder(&self, directory: SyncDirectory) {
        self.state.lock().await.new_folder(directory)
    }

    pub async fn set_new_folder_handler(&mut self, handler: NewFolderHandler) {
        self.state.lock().await.set_new_folder_handler(handler)
    }

    pub async fn add_peer_connection(&mut self, peer_connection: PeerConnection) {
        self.state.lock().await.add_peer_connection(peer_connection);
    }

    pub async fn directory_changed(&mut self, dir: &sync_directory::SyncDirectory) {
        self.state.lock().await.directory_changed(dir)
    }
}
