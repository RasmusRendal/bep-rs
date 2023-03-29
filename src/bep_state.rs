use super::models::*;
use super::sync_directory::*;
use diesel::prelude::*;
use std::fs;
use std::path::PathBuf;

use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};
pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("./migrations/");

/// This structure maintains the state of a bep
/// client
pub struct BepState {
    pub data_directory: PathBuf,
    connection: SqliteConnection,
}

impl BepState {
    /// Initialize the state. It tries to read a database
    /// from the location data_directory. If this fails,
    /// it creates a new database
    pub fn new(mut data_directory: PathBuf) -> Self {
        match data_directory.try_exists() {
            Ok(v) => {
                if !v {
                    if let Err(e) = fs::create_dir(&data_directory) {
                        panic!("Could not create data directory due to error {}", e);
                    }
                }
            }
            Err(_e) => {
                panic!("Could not access XDG CONFIG DIR");
            }
        }
        data_directory.push("db.sqlite");
        let mut connection = SqliteConnection::establish(data_directory.to_str().unwrap())
            .unwrap_or_else(|_| panic!("Error connecting to database"));
        if let Err(e) = connection.run_pending_migrations(MIGRATIONS) {
            panic!("Error when applying database migrations: {}", e);
        }
        BepState {
            data_directory,
            connection,
        }
    }

    /// Get list of directories to be synced
    pub fn get_sync_directories(&mut self) -> Vec<SyncDirectory> {
        use super::schema::sync_folders::dsl::*;
        sync_folders
            .load::<SyncFolder>(&mut self.connection)
            .unwrap_or_default()
            .iter()
            .map(|x| SyncDirectory {
                id: x.id.clone().unwrap(),
                label: x.label.clone(),
                path: PathBuf::from(x.dir_path.clone()),
            })
            .collect::<Vec<_>>()
    }

    /// Get a specific sync directory
    pub fn get_sync_directory(&mut self, id: String) -> Option<SyncDirectory> {
        // TODO: Handle this better
        for dir in self.get_sync_directories() {
            if dir.id == id {
                return Some(dir);
            }
        }
        None
    }

    fn get_options(&mut self) -> Option<DeviceOption> {
        use super::schema::device_options::dsl::*;
        device_options
            .load::<DeviceOption>(&mut self.connection)
            .unwrap_or_default()
            .pop()
    }

    pub fn get_name(&mut self) -> String {
        //TODO: Generate a random device name
        self.get_options()
            .map(|x| x.device_name)
            .unwrap_or("Device".to_string())
    }

    pub fn set_name(&mut self, name: String) {
        use crate::schema::device_options;
        use crate::schema::device_options::dsl;
        //use crate::schema::device_options;
        if let Some(mut options) = self.get_options() {
            options.device_name = name.clone();
            diesel::insert_into(device_options::table)
                .values(options)
                .on_conflict(dsl::id)
                .do_update()
                .set(dsl::device_name.eq(name))
                .execute(&mut self.connection)
                .unwrap();
        } else {
            let new_options = DeviceOption {
                id: Some(1),
                device_name: name,
            };
            diesel::insert_into(device_options::table)
                .values(&new_options)
                .execute(&mut self.connection)
                .expect("Error adding directory");
        }
    }

    /// Get list of peers to the client
    pub fn get_peers(&mut self) -> Vec<Peer> {
        use super::schema::peers::dsl::*;
        peers.load::<Peer>(&mut self.connection).unwrap_or_default()
    }

    /// Get the list of addressess associated wit hsome peer
    pub fn get_addresses(&mut self, peer: Peer) -> Vec<String> {
        use super::schema::peer_addresses::dsl::*;
        PeerAddress::belonging_to(&peer)
            .select(address)
            .load::<String>(&mut self.connection)
            .unwrap_or_default()
    }

    /// Stop syncing some directory
    pub fn remove_sync_directory(&mut self, to_remove: String) {
        use crate::schema::sync_folders::dsl::sync_folders;
        use crate::schema::sync_folders::id;

        diesel::delete(sync_folders.filter(id.eq(to_remove)))
            .execute(&mut self.connection)
            .expect("Error deleting folder");
    }

    /// Start syncing some directory
    pub fn add_sync_directory(&mut self, path: PathBuf, id: Option<String>) -> SyncDirectory {
        use crate::schema::sync_folders;
        use rand::distributions::{Alphanumeric, DistString};

        if !path.exists() {
            panic!("Folder {} not found", path.display());
        }

        let folder_id = id.unwrap_or(Alphanumeric.sample_string(&mut rand::thread_rng(), 12));

        let new_dir = SyncFolder {
            id: Some(folder_id.clone()),
            label: path.file_name().unwrap().to_owned().into_string().unwrap(),
            dir_path: path.display().to_string(),
        };

        diesel::insert_into(sync_folders::table)
            .values(&new_dir)
            .execute(&mut self.connection)
            .expect("Error adding directory");
        self.get_sync_directory(folder_id).unwrap()
    }

    pub fn add_peer(&mut self, peer_name: String) -> Peer {
        use crate::schema::peers;
        use crate::schema::peers::dsl::*;
        let p = Peer {
            id: None,
            name: peer_name.clone(),
        };
        diesel::insert_into(peers::table)
            .values(&p)
            .execute(&mut self.connection)
            .expect("Error adding peer");

        peers
            .filter(name.eq(peer_name))
            .limit(1)
            .load::<Peer>(&mut self.connection)
            .unwrap()
            .pop()
            .unwrap()
    }

    /// Allows the peer to request files from a directory
    pub fn sync_directory_with_peer(&mut self, directory: &SyncDirectory, peer: &Peer) {
        use crate::schema::folder_shares;
        let new_share = FolderShare {
            id: None,
            sync_folder_id: directory.id.clone(),
            peer_id: peer.id.unwrap(),
        };
        diesel::insert_into(folder_shares::table)
            .values(&new_share)
            .execute(&mut self.connection)
            .expect("Error syncing directory");
    }

    pub fn is_directory_synced(&mut self, directory: &SyncDirectory, peer: &Peer) -> bool {
        use crate::schema::folder_shares::dsl::*;

        folder_shares
            .filter(sync_folder_id.eq(directory.id.clone()))
            .filter(peer_id.eq(peer.id.unwrap()))
            .load::<FolderShare>(&mut self.connection)
            .map(|x| x.len() > 0)
            .unwrap_or(false)
    }
}
