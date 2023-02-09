use std::path::PathBuf;
use super::models::*;
use diesel::prelude::*;
use std::fs;

use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};
pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("./migrations/");

pub struct BepState {
    pub data_directory: PathBuf,
    connection: SqliteConnection,
}

impl BepState {
    pub fn new(mut data_directory: PathBuf) -> Self {
        match data_directory.try_exists() {
            Ok(v) => {
                if !v {
                    if let Err(e) = fs::create_dir(&data_directory) {
                        panic!("Could not create data directory due to error {}", e);
                    }

                }
            },
            Err(_e) => {
                panic!("Could not access XDG CONFIG DIR");
            }
        }
        data_directory.push("db.sqlite");
        let mut connection = SqliteConnection::establish(data_directory.to_str().unwrap()).unwrap_or_else(|_| panic!("Error connecting to database"));
        if let Err(e) = connection.run_pending_migrations(MIGRATIONS) {
            panic!("Error when applying database migrations: {}", e);
        }
        BepState { data_directory, connection }
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
