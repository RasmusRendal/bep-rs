use super::models::*;
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

/// A block of a file
#[derive(Clone)]
pub struct Block {
    pub offset: u64,
    pub size: u32,
    pub hash: Vec<u8>,
}

/// A file that we should be syncing
#[derive(Clone)]
pub struct File {
    pub name: String,
    pub hash: Vec<u8>,
    pub blocks: Vec<Block>,
    // If I were writing C++, it would be handly to always be able to refer back
    // to the parent when working with children of some objects. But that does
    // not agree well with the Rust memory model. So for the moment, I'll just
    // be passing a copy of the associated Directory with the File when needed
    //pub directory: Box<&Directory>,
}

/// A directory that we should be syncing
#[derive(Clone)]
pub struct Directory {
    pub id: String,
    pub label: String,
    pub path: PathBuf,
    pub files: Vec<File>,
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
    pub fn get_sync_directories(&mut self) -> Vec<Directory> {
        use super::schema::sync_folders::dsl::*;
        sync_folders
            .load::<SyncFolder>(&mut self.connection)
            .unwrap_or(Vec::new())
            .iter()
            .map(|x| Directory {
                id: "hello".to_string(),
                label: "hello".to_string(),
                path: PathBuf::from(x.dir_path.clone()),
                files: vec![],
            })
            .collect::<Vec<_>>()
    }

    /// Get list of peers to the client
    pub fn get_peers(&mut self) -> Vec<Peer> {
        use super::schema::peers::dsl::*;
        peers
            .load::<Peer>(&mut self.connection)
            .unwrap_or(Vec::new())
    }

    /// Get the list of addressess associated wit hsome peer
    pub fn get_addresses(&mut self, peer: Peer) -> Vec<String> {
        use super::schema::peer_addresses::dsl::*;
        PeerAddress::belonging_to(&peer)
            .select(address)
            .load::<String>(&mut self.connection)
            .unwrap_or(Vec::new())
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
    pub fn add_sync_directory(&mut self, path: PathBuf) {
        use crate::schema::sync_folders;
        use rand::distributions::{Alphanumeric, DistString};

        if !path.exists() {
            panic!("Folder {} not found", path.display());
        }

        let folder_id: String = Alphanumeric.sample_string(&mut rand::thread_rng(), 12);

        let new_dir = SyncFolder {
            id: Some(folder_id),
            label: path.file_name().unwrap().to_owned().into_string().unwrap(),
            dir_path: path.display().to_string(),
        };

        diesel::insert_into(sync_folders::table)
            .values(&new_dir)
            .execute(&mut self.connection)
            .expect("Error adding directory");
    }
}
