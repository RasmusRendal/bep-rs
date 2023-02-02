use std::path::PathBuf;
use super::models::*;
use diesel::prelude::*;

pub struct BepState {
    pub data_directory: PathBuf,
    connection: SqliteConnection,
}

impl BepState {
    pub fn new(mut data_directory: PathBuf) -> Self {
        data_directory.push("db.sqlite");
        let connection = SqliteConnection::establish(data_directory.to_str().unwrap()).unwrap_or_else(|_| panic!("Error connecting to database"));
        BepState { data_directory: data_directory, connection: connection }
    }

    pub fn get_sync_directories(&mut self) -> Vec<Syncfolder> {
        use super::schema::syncfolders::dsl::*;
        syncfolders.load::<Syncfolder>(&mut self.connection).unwrap_or(Vec::new())
    }

    pub fn get_peers(&mut self) -> Vec<Peer> {
        use super::schema::peers::dsl::*;
        peers.load::<Peer>(&mut self.connection).unwrap_or(Vec::new())
    }

    pub fn get_addresses(&mut self, peer: Peer) -> Vec<String> {
        use super::schema::peer_addresses::dsl::*;
        PeerAddress::belonging_to(&peer)
            .select(address)
            .load::<String>(&mut self.connection).unwrap_or(Vec::new())
    }

    pub fn remove_sync_directory(&mut self, to_remove: String) {
        use crate::schema::syncfolders::dsl::syncfolders;
        use crate::schema::syncfolders::id;

        diesel::delete(syncfolders.filter(id.eq(to_remove)))
            .execute(& mut self.connection)
            .expect("Error deleting folder");
    }

    pub fn add_sync_directory(&mut self, path: PathBuf) {
        use rand::distributions::{Alphanumeric, DistString};
        use crate::schema::syncfolders;

        if !path.exists() {
            panic!("Folder {} not found", path.display());
        }

        let folder_id: String = Alphanumeric.sample_string(&mut rand::thread_rng(), 12);

        let new_dir = Syncfolder {
            id: Some(folder_id),
            label: path.file_name().unwrap().to_owned().into_string().unwrap(),
            dir_path: path.display().to_string()
        };

        diesel::insert_into(syncfolders::table)
            .values(&new_dir)
            .execute(&mut self.connection)
            .expect("Error adding directory");

    }

}
